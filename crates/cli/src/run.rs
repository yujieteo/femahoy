//! `fh run`: solve a model on the daemon of the run contract (`docs/run-contract.md`), show the
//! progress, and open the results in the browser.
//!
//! - `fh run MODEL.inp` solves the input as it is.
//! - `fh run CASE_DIR` (or `CASE_DIR/case.toml`, or `CASE_DIR/PART.step`) takes a case folder
//!   with `case.toml` and one STEP file. It runs `fh mesh` and `fh model` in the work folder
//!   `<state>/cases/<name>-<hash>`, then solves the input. Thus the case folder does not
//!   change.
//! - Each run uses the solver of the daemon: `FH_SOLVE` when set, else `fh-solve-ccx` beside
//!   `fh` or on `PATH`. `fh-solve-ccx` needs CalculiX (`docs/ccx.md`).
//! - `fh run --example NAME` writes an example case that `fh` carries to
//!   `<state>/examples/NAME`, then runs it as a case folder. It works from any folder.
//!
//! Before the solve, `fh run` sends the input to the preview route of the daemon, so that
//! the page shows the model with each support and each load. With `--preview`, `fh run`
//! stops there and does not solve.
//!
//! Which process stays alive: `fh run` uses the daemon of the state folder when one answers.
//! Else it starts `fh serve --state DIR --no-open` in the background, in its own process
//! group, with its output in `DIR/daemon.log`. That daemon stays alive when `fh run` ends,
//! so that the browser keeps the results, and the next `fh run` uses it.
//! `fh serve --stop` stops it.
//!
//! Ctrl+C sends a cancel command for the run and waits for the end of the run. A second
//! Ctrl+C ends `fh run` at once. The cancel continues in the daemon.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use fh_daemon::{Client, Event, RunRecord, RunState};

const USAGE: &str = "usage: fh run MODEL.inp|CASE_DIR|PART.step [--size H] \
[--preview] [--state DIR] [--no-open]\n       fh run --example NAME [--size H] \
[--preview] [--state DIR] [--no-open]";

/// Exit codes of `fh run`, in addition to 0 (the run succeeded) and 2 (usage error).
const FAILED: u8 = 1;
const NO_DAEMON: u8 = 3;
const CANCELLED: u8 = 130;

/// The file in the state folder that gets the output of a daemon that `fh run` starts.
const LOG_FILE: &str = "daemon.log";
/// The longest time to wait for a new daemon to answer.
const START_WAIT: Duration = Duration::from_secs(20);
/// The time to wait for another daemon after the new one ended, because another `fh run` can
/// start a daemon on the same state folder at the same time and win its lock.
const LOSER_WAIT: Duration = Duration::from_secs(3);
/// The longest time without an answer of the daemon during a run.
const LOST_WAIT: Duration = Duration::from_secs(10);
/// The wait of each events request. It is short, so that Ctrl+C acts fast.
const EVENT_WAIT: Duration = Duration::from_millis(500);

/// An example case that `fh` carries: the files of a validation case and its mesh size.
struct Example {
    name: &'static str,
    about: &'static str,
    size: &'static str,
    files: &'static [(&'static str, &'static [u8])],
}

const EXAMPLES: &[Example] = &[Example {
    name: "bar-pressure",
    about: "a steel bar of 100 x 20 x 10 mm, pulled by a pressure of 100 MPa on one end face. \
            The MISES stress is 100 MPa in all of the bar",
    size: "8",
    files: &[
        (
            "case.toml",
            include_bytes!("../../../validation/cases/bar-pressure/case.toml"),
        ),
        (
            "bar.step",
            include_bytes!("../../../validation/cases/bar-pressure/bar.step"),
        ),
        (
            "derivation.md",
            include_bytes!("../../../validation/cases/bar-pressure/derivation.md"),
        ),
    ],
}];

#[derive(Default)]
struct Options {
    target: Option<PathBuf>,
    example: Option<String>,
    size: Option<String>,
    state: Option<PathBuf>,
    no_open: bool,
    preview: bool,
}

fn example_names() -> String {
    EXAMPLES
        .iter()
        .map(|e| e.name)
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs {what}\n{USAGE}"))
        };
        match arg.as_str() {
            "--example" => o.example = Some(value(&format!("a name: {}", example_names()))?),
            "--size" => {
                let size = value("a mesh size")?;
                if !size.parse::<f64>().is_ok_and(|h| h.is_finite() && h > 0.0) {
                    return Err(format!("--size needs a number above 0\n{USAGE}"));
                }
                o.size = Some(size);
            }
            "--state" => o.state = Some(value("a folder")?.into()),
            "--no-open" => o.no_open = true,
            "--preview" => o.preview = true,
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s if o.target.is_none() => o.target = Some(s.into()),
            s => return Err(format!("unexpected argument: {s}\n{USAGE}")),
        }
    }
    match (&o.target, &o.example) {
        (None, None) => Err(USAGE.into()),
        (Some(_), Some(_)) => Err(format!("give a model or --example, not both\n{USAGE}")),
        _ => Ok(o),
    }
}

/// What `fh run` solves.
enum Input {
    Inp(PathBuf),
    Case { dir: PathBuf, step: PathBuf },
}

fn is_step(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    matches!(ext.as_deref(), Some("step" | "stp"))
}

/// The input of `path`: an `.inp` file, or a case folder with its STEP file.
fn resolve(path: &Path) -> Result<Input, String> {
    let shown = path.display();
    let full = fs::canonicalize(path).map_err(|e| format!("{shown}: {e} (run.input.missing)"))?;
    if full.is_dir() {
        return case_in(full, None);
    }
    let parent = full.parent().map(Path::to_path_buf).unwrap_or_default();
    let ext = full
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if ext.as_deref() == Some("inp") {
        Ok(Input::Inp(full))
    } else if is_step(&full) {
        case_in(parent, Some(full))
    } else if full.file_name().is_some_and(|n| n == "case.toml") {
        case_in(parent, None)
    } else {
        Err(format!(
            "{shown}: fh run takes an .inp file, a case folder, its case.toml or its STEP file \
             (run.input.kind)"
        ))
    }
}

fn case_in(dir: PathBuf, step: Option<PathBuf>) -> Result<Input, String> {
    let shown = dir.display();
    if !dir.join("case.toml").is_file() {
        return Err(format!(
            "{shown}: no case.toml, so this is not a case folder (run.case.missing)"
        ));
    }
    let step = match step {
        Some(step) => step,
        None => {
            let entries = fs::read_dir(&dir).map_err(|e| format!("{shown}: {e}"))?;
            let mut steps: Vec<PathBuf> = entries
                .filter_map(|e| Some(e.ok()?.path()))
                .filter(|p| p.is_file() && is_step(p))
                .collect();
            steps.sort();
            match steps.len() {
                1 => steps.remove(0),
                0 => {
                    return Err(format!(
                        "{shown}: the case folder has no STEP file (.step or .stp) \
                         (run.case.step.missing)"
                    ))
                }
                n => {
                    let names: Vec<_> = steps.iter().filter_map(|p| p.file_name()).collect();
                    return Err(format!(
                        "{shown}: the case folder has {n} STEP files ({}). Give one of them, \
                         for example: fh run {} (run.case.step.many)",
                        names
                            .iter()
                            .map(|n| n.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join(", "),
                        steps[0].display()
                    ));
                }
            }
        }
    };
    Ok(Input::Case { dir, step })
}

fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: the path is not UTF-8", path.display()))
}

/// Write the files of an example to `<state>/examples/NAME` and give the folder.
fn write_example(state: &Path, example: &Example) -> Result<PathBuf, String> {
    let dir = state.join("examples").join(example.name);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    for (name, bytes) in example.files {
        let path = dir.join(name);
        fs::write(&path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    println!(
        "fh run: example {}: {}.\nfh run: the files of the example are in {}",
        example.name,
        example.about,
        dir.display()
    );
    Ok(dir)
}

/// The tail of the daemon log, for an error message.
fn log_tail(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(5)..]
        .iter()
        .map(|l| format!("\n  {l}"))
        .collect()
}

fn answers(state: &Path) -> Option<Client> {
    Client::discover(state).ok().filter(|c| c.status().is_ok())
}

/// The daemon of the state folder: the daemon that answers, else a new `fh serve` in the
/// background. The flag is true for a new daemon.
fn daemon(state: &Path) -> Result<(Client, bool), String> {
    if let Some(client) = answers(state) {
        return Ok((client, false));
    }
    let log_path = state.join(LOG_FILE);
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let log = options
        .open(&log_path)
        .map_err(|e| format!("cannot write {}: {e}", log_path.display()))?;
    fs::set_permissions(
        &log_path,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .map_err(|e| format!("cannot protect {}: {e}", log_path.display()))?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the fh program: {e}"))?;
    let mut command = Command::new(exe);
    command
        .arg("serve")
        .arg("--state")
        .arg(state)
        .arg("--no-open")
        .current_dir(state)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(err);
    // Its own process group: a Ctrl+C in the terminal stops fh run, not the daemon.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot start fh serve: {e}"))?;
    let deadline = Instant::now() + START_WAIT;
    let mut ended: Option<Instant> = None;
    loop {
        if let Some(client) = answers(state) {
            // Another fh run can start the daemon that answers. Then stop the new one now:
            // it tries the lock for 1 s, and it must not take the lock when the other daemon
            // stops. It has no lock and no run, because the daemon that answers has the lock.
            let pid = client
                .status()
                .ok()
                .and_then(|m| m.get("pid")?.parse::<u32>().ok());
            let new = pid == Some(child.id());
            if !new {
                let _ = child.kill();
                let _ = child.wait();
            }
            return Ok((client, new));
        }
        if ended.is_none() && child.try_wait().ok().flatten().is_some() {
            ended = Some(Instant::now());
        }
        if ended.is_some_and(|t| t.elapsed() > LOSER_WAIT) || Instant::now() > deadline {
            let _ = child.kill();
            return Err(format!(
                "fh serve did not start a daemon for {}. The end of {}:{}",
                state.display(),
                log_path.display(),
                log_tail(&log_path)
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The FNV-1a hash of the path, for the name of a work folder.
fn path_hash(path: &Path) -> u64 {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Run `fh mesh` and `fh model` for a case in its work folder. The result is the input file,
/// or the exit code of the step that refused the case.
fn prepare(
    work: &Path,
    dir: &Path,
    step: &Path,
    size: Option<&str>,
) -> Result<Result<PathBuf, ExitCode>, String> {
    let stem = step
        .file_stem()
        .map_or("part".into(), |s| s.to_string_lossy());
    let msh = work.join(format!("{stem}.msh"));
    let inp = work.join(format!("{stem}.inp"));
    let mut mesh = vec![
        text(step)?,
        "--case".into(),
        text(dir)?,
        "-o".into(),
        text(&msh)?,
    ];
    if let Some(size) = size {
        mesh.extend(["--size".into(), size.into()]);
    }
    println!("fh run: mesh: fh mesh {}", mesh.join(" "));
    let code = crate::mesh::run(&mesh).map_err(|e| format!("mesh: {e}"))?;
    if code != ExitCode::SUCCESS {
        return Ok(Err(code));
    }
    let model = [
        text(&msh)?,
        "--case".into(),
        text(dir)?,
        "-o".into(),
        text(&inp)?,
    ];
    println!("fh run: model: fh model {}", model.join(" "));
    let code = crate::model::run(&model).map_err(|e| format!("model: {e}"))?;
    if code != ExitCode::SUCCESS {
        return Ok(Err(code));
    }
    Ok(Ok(inp))
}

fn fail(code: u8, message: &str) -> ExitCode {
    eprintln!("fh run: {message}");
    ExitCode::from(code)
}

/// One event of the run as a line for a person.
fn show(event: &Event) -> String {
    let text = &event.text;
    // `fh run` shows the reason of a failed run at its end.
    if let Some(state) = text.strip_prefix("state ") {
        return state.split(':').next().unwrap_or(state).to_owned();
    }
    if let Some(bytes) = text.strip_prefix("estimate bytes=") {
        let mib = bytes.parse::<f64>().unwrap_or(0.0) / f64::from(1 << 20);
        return format!("memory estimate {mib:.1} MiB");
    }
    if let Some(rest) = text.strip_prefix("progress ") {
        let field = |key: &str| {
            rest.split(' ')
                .find_map(|f| f.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("?")
        };
        let percent = field("fraction").parse::<f64>().map_or(0.0, |f| f * 100.0);
        return format!("step {} {} {percent:.0}%", field("step"), field("phase"));
    }
    text.strip_prefix("log ").unwrap_or(text).to_owned()
}

/// Show the events of the run until it ends. A Ctrl+C sends a cancel command, a second one
/// ends `fh run`. `None` means that `fh run` ends before the run.
fn follow(client: &Client, run: &str) -> Result<Option<RunState>, String> {
    let mut since = 0;
    let (mut cancel, mut cancel_sent) = (false, false);
    let mut lost: Option<Instant> = None;
    loop {
        let hits = interrupt::count();
        if hits >= 2 {
            println!(
                "fh run: Ctrl+C again: fh run ends. The cancel of {run} continues in the daemon."
            );
            return Ok(None);
        }
        if hits == 1 && !cancel {
            println!("fh run: Ctrl+C: cancel {run}. Press Ctrl+C again to end fh run at once.");
            cancel = true;
        }
        if cancel && !cancel_sent {
            match client.cancel(run) {
                Ok(record) => {
                    cancel_sent = true;
                    println!("  cancel: {}", record.state);
                }
                // The next pass sends the cancel again.
                Err(e) if e.status == 0 => {}
                Err(e) => {
                    cancel_sent = true;
                    eprintln!("fh run: the cancel failed: {e}");
                }
            }
        }
        let events = match client.events(run, since, EVENT_WAIT) {
            Ok(events) => events,
            Err(e) if e.status == 0 => {
                let first = *lost.get_or_insert_with(Instant::now);
                if first.elapsed() > LOST_WAIT {
                    return Err(format!("the daemon does not answer: {e}"));
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        lost = None;
        if events.dropped > 0 {
            println!("  ({} events are no longer in the queue)", events.dropped);
        }
        for event in &events.items {
            println!("  {}", show(event));
        }
        since = events
            .items
            .last()
            .map_or(since.max(events.last), |e| e.seq);
        if events.state.is_final() && since >= events.last {
            return Ok(Some(events.state));
        }
    }
}

/// The message of a run, with the path of the copy of the input replaced by the input file.
fn message_of(record: &RunRecord, state: &Path, input: &Path) -> String {
    let copy = state.join("runs").join(&record.run).join("input.inp");
    record
        .message
        .replace(&copy.display().to_string(), &input.display().to_string())
}

/// Print the result folder and the summary of a run that succeeded, and open the browser.
fn report(client: &Client, record: &RunRecord, state: &Path, o: &Options, stop_hint: &str) {
    let out = state.join("runs").join(&record.run).join("out");
    let seconds = record.finished_ms.saturating_sub(record.started_ms) as f64 / 1000.0;
    println!("fh run: {} succeeded in {seconds:.2} s", record.run);
    println!("fh run: results in {}", out.display());
    match fs::read_to_string(out.join("summary.toon")) {
        Ok(summary) => {
            println!("fh run: summary.toon:");
            for line in summary.lines() {
                println!("  {line}");
            }
        }
        Err(e) => println!("fh run: no summary.toon ({e})"),
    }
    let url = client.page_url(&format!("&run={}", record.run));
    println!("fh run: page {url}");
    if !fh_daemon::no_open(o.no_open) {
        match fh_daemon::open_page(state, &url) {
            Ok(()) => println!("fh run: opened {} in the browser", record.run),
            Err(e) => println!("fh run: no browser ({e}). Open the page above."),
        }
    }
    println!("fh run: the daemon stays alive for the browser. Stop it with: {stop_hint}");
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let state = o
        .state
        .clone()
        .or_else(fh_daemon::default_state_dir)
        .ok_or_else(|| format!("no home folder, so give the state folder\n{USAGE}"))?;
    let example = match &o.example {
        Some(name) => Some(EXAMPLES.iter().find(|e| e.name == name).ok_or_else(|| {
            format!(
                "unknown example {name}. The examples: {} (run.example.unknown)",
                example_names()
            )
        })?),
        None => None,
    };
    // Check the model before a daemon starts.
    let input = match &o.target {
        Some(target) => Some(resolve(target)?),
        None => None,
    };
    if matches!(input, Some(Input::Inp(_))) && o.size.is_some() {
        return Err(format!(
            "--size goes to fh mesh, for a case folder or a STEP file. An .inp file \
             has its mesh (run.option.mesh)\n{USAGE}"
        ));
    }
    if let Err(e) = fs::create_dir_all(&state) {
        let message = format!(
            "no daemon: cannot make the state folder {}: {e}",
            state.display()
        );
        return Ok(fail(NO_DAEMON, &message));
    }
    // The canonical path: the daemon gives it in its paths, and a new daemon runs in the
    // state folder, where a relative path names another folder.
    let state = fs::canonicalize(&state).unwrap_or(state);
    let input = match (input, example) {
        (Some(input), _) => input,
        (None, Some(example)) => resolve(&write_example(&state, example)?)?,
        (None, None) => unreachable!("parse needs a model or an example"),
    };
    // Without FH_SOLVE the daemon runs fh-solve-ccx. Stop now when it is not there.
    let solver = fh_daemon::SOLVER;
    if std::env::var_os("FH_SOLVE").is_none()
        && fh_daemon::find_solver(&fh_daemon::solver_dirs(), solver).is_none()
    {
        return Ok(fail(FAILED, &fh_daemon::missing_solver(solver)));
    }
    let (client, started) = match daemon(&state) {
        Ok(found) => found,
        Err(e) => return Ok(fail(NO_DAEMON, &format!("no daemon: {e}"))),
    };
    let stop_hint = match &o.state {
        Some(_) => format!("fh serve --stop --state {}", state.display()),
        None => "fh serve --stop".to_owned(),
    };
    if started {
        println!(
            "fh run: started a daemon for {} (log {})",
            state.display(),
            state.join(LOG_FILE).display()
        );
    }
    // The name of the model in the page: the case folder, else the input file.
    let (file, case) = match &input {
        Input::Inp(file) => (file.clone(), None),
        Input::Case { dir, step } => {
            let name = dir
                .file_name()
                .map_or("case".into(), |n| n.to_string_lossy());
            let work = state
                .join("cases")
                .join(format!("{name}-{:016x}", path_hash(dir)));
            fs::create_dir_all(&work)
                .map_err(|e| format!("cannot make {}: {e}", work.display()))?;
            // Two fh run commands on one case at the same time take turns.
            let lock = File::create(work.join(".lock")).and_then(|f| f.lock().map(|()| f));
            let _lock = lock.map_err(|e| format!("cannot lock {}: {e}", work.display()))?;
            let size = o.size.as_deref().or(example.map(|e| e.size));
            match prepare(&work, dir, step, size)? {
                Ok(inp) => (inp, Some(name.into_owned())),
                Err(code) => return Ok(code),
            }
        }
    };
    let title = match case {
        Some(case) => case,
        None => text(Path::new(file.file_name().unwrap_or_default()))?,
    };
    interrupt::install();
    // The page shows the model before the solve. A daemon with no preview route still solves.
    match (client.preview(&file, &title), o.preview) {
        (Ok(preview), _) => println!(
            "fh run: preview: the page shows {title} (preview {preview}) with its supports and \
             loads"
        ),
        (Err(e), false) => println!("fh run: no preview ({e})"),
        (Err(e), true) => {
            return Ok(fail(
                FAILED,
                &format!("the daemon refused the preview: {e}"),
            ))
        }
    }
    if o.preview {
        println!("fh run: --preview: no solve");
        return Ok(ExitCode::SUCCESS);
    }
    let run = match client.submit(&file) {
        Ok(record) => record.run,
        Err(e) if e.status == 0 => return Ok(fail(NO_DAEMON, &format!("no daemon: {e}"))),
        Err(e) => return Ok(fail(FAILED, &format!("the daemon refused the run: {e}"))),
    };
    println!("fh run: {run} solves {}", file.display());
    let end = match follow(&client, &run) {
        Ok(Some(end)) => end,
        Ok(None) => return Ok(ExitCode::from(CANCELLED)),
        Err(e) => return Ok(fail(NO_DAEMON, &format!("{run}: {e}"))),
    };
    let record = match client.record(&run) {
        Ok(record) => record,
        Err(e) => return Ok(fail(NO_DAEMON, &format!("{run}: {e}"))),
    };
    let message = message_of(&record, &state, &file);
    Ok(match end {
        RunState::Succeeded => {
            report(&client, &record, &state, &o, &stop_hint);
            ExitCode::SUCCESS
        }
        RunState::Cancelled => fail(CANCELLED, &format!("{run} was cancelled")),
        RunState::Interrupted => fail(FAILED, &format!("{run} was interrupted: {message}")),
        _ => fail(FAILED, &format!("{run} failed: {message}")),
    })
}

/// Ctrl+C (SIGINT) during a run. The handler only counts. The run loop reads the count.
mod interrupt {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNT: AtomicUsize = AtomicUsize::new(0);

    pub fn count() -> usize {
        COUNT.load(Ordering::SeqCst)
    }

    pub fn install() {
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn on_signal(_: i32) {
            COUNT.fetch_add(1, Ordering::SeqCst);
        }
        // SAFETY: the handler only adds to an atomic counter, which is async-signal-safe.
        // SIGINT is 2 on Linux and macOS.
        unsafe {
            signal(2, on_signal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_as_lines_for_a_person() {
        let line = |text: &str| {
            show(&Event {
                seq: 1,
                text: text.into(),
            })
        };
        assert_eq!(line("state running"), "running");
        assert_eq!(line("estimate bytes=2097152"), "memory estimate 2.0 MiB");
        assert_eq!(
            line("progress step=1/2 phase=assemble fraction=0.5"),
            "step 1/2 assemble 50%"
        );
        assert_eq!(line("log step 1 max_u=1"), "step 1 max_u=1");
    }

    #[test]
    fn options_are_checked() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse(&args(&["m.inp", "--size", "0"])).is_err());
        assert!(parse(&args(&["m.inp", "--order", "2"])).is_err());
        assert!(parse(&args(&["m.inp", "--example", "bar-pressure"])).is_err());
        assert!(parse(&args(&[])).is_err());
        let o = parse(&args(&["--example", "bar-pressure", "--no-open"])).unwrap();
        assert!(o.no_open && o.example.as_deref() == Some("bar-pressure"));
    }

    #[test]
    fn the_example_files_make_a_case_folder() {
        let state = std::env::temp_dir().join(format!("fh-run-example-{}", std::process::id()));
        let dir = write_example(&state, &EXAMPLES[0]).unwrap();
        let Input::Case { step, .. } = resolve(&dir).unwrap() else {
            panic!("not a case folder");
        };
        assert_eq!(step.file_name().unwrap(), "bar.step");
        fh_validation::Case::load_before_mesh(&dir).unwrap();
        let _ = fs::remove_dir_all(state);
    }
}
