//! `fh run`: solve a CalculiX input on the daemon and show the progress.
//!
//! - `fh run MODEL.inp` gives CalculiX the input as it is, with each file that it includes
//!   (`*INCLUDE`). The daemon solves a copy, so a later edit does not change the run.
//! - The daemon solves each run with CalculiX: `$FH_CCX`, else `ccx` on `PATH`.
//! - `--timeout SECONDS` and `--memory MIB` set the limits of the run.
//!
//! Before the solve, `fh run` sends the input to the preview route of the daemon, so that
//! the page shows the model with each support and each load. With `--preview`, `fh run`
//! stops there and does not solve.
//!
//! `fh run` needs the daemon of `fh serve`: it finds it in the state folder (`$FH_STATE`).
//!
//! Ctrl+C sends a cancel command for the run and waits for the end of the run. A second
//! Ctrl+C ends `fh run` at once. The cancel continues in the daemon.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use fh_daemon::{Client, Event, Limits, RunRecord, RunState};

const USAGE: &str = "usage: fh run MODEL.inp [--preview] [--timeout SECONDS] [--memory MIB]";

/// Exit codes of `fh run`, in addition to 0 (the run succeeded) and 2 (usage error).
const FAILED: u8 = 1;
const NO_DAEMON: u8 = 3;
const CANCELLED: u8 = 130;

/// The longest time without an answer of the daemon during a run.
const LOST_WAIT: Duration = Duration::from_secs(10);
/// The wait of each events request. It is short, so that Ctrl+C acts fast.
const EVENT_WAIT: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Options {
    target: Option<PathBuf>,
    limits: Limits,
    preview: bool,
}

/// A number above 0 that is finite.
fn positive(value: &str, what: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| format!("{what} needs a number above 0\n{USAGE}"))
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
            "--timeout" => {
                let seconds = positive(&value("a number of seconds")?, "--timeout")?;
                o.limits.timeout = Some(
                    Duration::try_from_secs_f64(seconds)
                        .map_err(|_| format!("--timeout is too large\n{USAGE}"))?,
                );
            }
            "--memory" => {
                let mib = positive(&value("a number of MiB")?, "--memory")?;
                o.limits.memory = Some((mib * f64::from(1 << 20)) as u64);
            }
            "--preview" => o.preview = true,
            s if s.starts_with('-') => return Err(format!("unknown option: {s}\n{USAGE}")),
            s if o.target.is_none() => o.target = Some(s.into()),
            s => return Err(format!("unexpected argument: {s}\n{USAGE}")),
        }
    }
    if o.target.is_none() {
        return Err(USAGE.into());
    }
    Ok(o)
}

/// The real path of the `.inp` file that `fh run` solves.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = path.display();
    let full = fs::canonicalize(path).map_err(|e| format!("{shown}: {e} (run.input.missing)"))?;
    let inp = full
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("inp"));
    if full.is_file() && inp {
        Ok(full)
    } else {
        Err(format!(
            "{shown}: fh run takes a CalculiX input file (.inp) (run.input.kind)"
        ))
    }
}

fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{}: the path is not UTF-8", path.display()))
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
    if let Some(rest) = text.strip_prefix("progress ") {
        return rest.replace('=', " ");
    }
    text.to_owned()
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
    let copy = state.join("runs").join(&record.run).join("in/input.inp");
    record
        .message
        .replace(&copy.display().to_string(), &input.display().to_string())
}

/// The number of result frames that `results.pvd` lists.
fn frames(out: &Path) -> usize {
    fs::read_to_string(out.join("results.pvd")).map_or(0, |t| t.matches("<DataSet").count())
}

/// Print the wall time and the result folder of a run that succeeded.
fn report(record: &RunRecord, state: &Path) {
    let out = state.join("runs").join(&record.run).join("out");
    let seconds = record.finished_ms.saturating_sub(record.started_ms) as f64 / 1000.0;
    println!("fh run: {} succeeded in {seconds:.2} s", record.run);
    println!("fh run: results in {}", out.display());
    println!(
        "fh run: text results {}/ccx/job.dat (print requests), {}/ccx/job.sta (steps)",
        out.display(),
        out.display()
    );
    if frames(&out) == 0 {
        println!(
            "fh run: the page has no field to show. Add *NODE FILE or *EL FILE to a *STEP to \
             write a field"
        );
    }
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let state = fh_daemon::default_state_dir()
        .ok_or_else(|| format!("no home folder, so set FH_STATE\n{USAGE}"))?;
    // Check the model before the daemon is asked.
    let file = resolve(o.target.as_deref().expect("parse needs a model"))?;
    let client = match Client::discover(&state).and_then(|c| c.status().map(|_| c)) {
        Ok(client) => client,
        Err(e) => {
            let message = format!("no daemon: {e}. Start it with `fh serve`");
            return Ok(fail(NO_DAEMON, &message));
        }
    };
    // The name of the model in the page: the input file.
    let title = text(Path::new(file.file_name().unwrap_or_default()))?;
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
    let run = match client.submit(&file, &o.limits) {
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
            report(&record, &state);
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
        assert_eq!(line("state failed: ccx failed"), "failed");
    }

    #[test]
    fn options_are_checked() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse(&args(&["m.inp", "--timeout", "0"])).is_err());
        assert!(parse(&args(&["m.inp", "--memory", "x"])).is_err());
        assert!(parse(&args(&["m.inp", "--size", "1"])).is_err());
        assert!(parse(&args(&[])).is_err());
        let o = parse(&args(&["m.inp", "--timeout", "1.5", "--memory", "512"])).unwrap();
        assert_eq!(o.limits.timeout, Some(Duration::from_millis(1500)));
        assert_eq!(o.limits.memory, Some(512 << 20));
    }
}
