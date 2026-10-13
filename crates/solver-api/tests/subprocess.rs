//! Protocol v2 runner tests with small shell scripts as fake solvers.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fh_solver_api::{CancelToken, Event, Registry, RunReport, Solver, SubprocessSolver};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fh-solver-api-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn script(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("solver.sh");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let inp = dir.join("a.inp");
    fs::write(&inp, "*NODE\n").unwrap();
    path
}

const SUMMARY: &str = "schema: fh.run\nversion: 1.0.0\nstatus: ok\n";

const GOOD: &str = r#"echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=0.5"
echo "restart restart.txt"
echo '<VTKFile type="Collection"><Collection><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>' > results.pvd
echo "<VTKFile/>" > step-1.vtu
echo restart > restart.txt
printf 'schema: fh.run\nversion: 1.0.0\nstatus: ok\n' > summary.toon"#;

fn run(name: &str, body: &str) -> Result<RunReport, String> {
    let dir = scratch(name);
    let exe = script(&dir, body);
    SubprocessSolver::new(exe)
        .run(&dir.join("a.inp"), &dir.join("run"))
        .map_err(|e| e.to_string())
}

#[test]
fn good_solver_passes() {
    let r = run("good", GOOD).unwrap();
    assert!(r.pvd.ends_with("results.pvd"));
    assert_eq!(r.summary, SUMMARY);
    assert!(!r.validated);
}

/// `to` as a path relative to the folder of the test process.
fn relative_to_cwd(to: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap();
    let common = cwd
        .components()
        .zip(to.components())
        .take_while(|(a, b)| a == b)
        .count();
    let mut rel: PathBuf = cwd.components().skip(common).map(|_| "..").collect();
    rel.extend(to.components().skip(common));
    rel
}

#[test]
fn a_relative_solver_path_resolves_from_the_caller_folder() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fh-solver-api-relative-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let exe = relative_to_cwd(&script(&dir, GOOD));
    assert!(exe.is_relative() && exe.is_file(), "{}", exe.display());
    let r = SubprocessSolver::new(exe).run(&dir.join("a.inp"), &dir.join("run"));
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn missing_header_is_rejected() {
    let e = run(
        "nohead",
        &GOOD.replace("echo \"fh-solver-protocol 2\"", "echo hello"),
    )
    .unwrap_err();
    assert!(e.contains("does not speak protocol v2"), "{e}");
}

#[test]
fn failure_reports_stderr() {
    let e = run("fail", "echo 'bad mesh' >&2; exit 3").unwrap_err();
    assert!(e.contains("solver failed") && e.contains("bad mesh"), "{e}");
}

#[test]
fn listed_file_must_exist() {
    let e = run(
        "nofile",
        &GOOD.replace("echo \"<VTKFile/>\" > step-1.vtu", "true"),
    )
    .unwrap_err();
    assert!(e.contains("step-1.vtu"), "{e}");
}

#[test]
fn time_limit_kills_the_solver() {
    let dir = scratch("slow");
    let exe = script(&dir, "sleep 30");
    let mut s = SubprocessSolver::new(exe);
    s.timeout = Duration::from_millis(200);
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("time limit"), "{e}");
}

#[test]
fn memory_limit_kills_the_solver() {
    // awk builds a string of about 134 MB and then loops. The script replaces
    // itself with awk, so the process that the runner watches holds the memory.
    let dir = scratch("memory");
    let exe = script(
        &dir,
        r#"exec awk 'BEGIN { s = "x"; for (i = 0; i < 27; i++) s = s s; while (1) { } }'"#,
    );
    let mut s = SubprocessSolver::new(exe);
    s.memory_limit = 50 << 20;
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("memory limit"), "{e}");
}

fn solver(dir: &Path, body: &str) -> SubprocessSolver {
    SubprocessSolver::new(script(dir, body))
}

#[test]
fn events_arrive_in_order() {
    let dir = scratch("events");
    let s = solver(&dir, GOOD);
    let mut seen = Vec::new();
    let r = s
        .run_with(
            &dir.join("a.inp"),
            &dir.join("run"),
            &CancelToken::new(),
            &mut |e| {
                seen.push(e.clone());
            },
        )
        .unwrap();
    assert_eq!(
        seen,
        vec![
            Event::Progress {
                step: 1,
                steps: 1,
                phase: "solve".into(),
                fraction: 0.5
            },
            Event::Restart("restart.txt".into()),
        ]
    );
    assert!(r.restart.unwrap().ends_with("restart.txt"));
}

#[test]
fn a_log_line_that_is_not_utf8_does_not_stop_the_events() {
    let dir = scratch("latin1");
    // A Latin-1 log line, for example a file name from a library, comes before the events.
    let body = GOOD.replacen('\n', "\nprintf 'caf\\351 au lait\\n'\n", 1);
    let s = solver(&dir, &body);
    let mut seen = Vec::new();
    let r = s.run_with(
        &dir.join("a.inp"),
        &dir.join("run"),
        &CancelToken::new(),
        &mut |e| seen.push(e.clone()),
    );
    let r = r.unwrap();
    assert_eq!(
        seen.first(),
        Some(&Event::Log("caf\u{FFFD} au lait".into()))
    );
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(r.restart.unwrap().ends_with("restart.txt"));
    let log = fs::read(dir.join("run/stdout.txt")).unwrap();
    assert!(String::from_utf8_lossy(&log).contains("caf\u{FFFD} au lait"));
}

#[test]
fn event_lines_parse() {
    assert_eq!(
        Event::parse("estimate bytes=1048576"),
        Event::Estimate { bytes: 1 << 20 }
    );
    assert_eq!(Event::parse("hello"), Event::Log("hello".into()));
    assert_eq!(
        Event::parse("progress step=x"),
        Event::Log("progress step=x".into())
    );
}

#[test]
fn a_progress_line_outside_the_protocol_range_is_a_log() {
    let mut bad = Vec::new();
    for line in [
        "progress step=9/0 phase=solve fraction=NaN",
        "progress step=1/1 phase=solve fraction=NaN",
        "progress step=1/1 phase=solve fraction=inf",
        "progress step=1/1 phase=solve fraction=-0.1",
        "progress step=1/1 phase=solve fraction=1.5",
        "progress step=1/0 phase=solve fraction=0.5",
        "progress step=0/2 phase=solve fraction=0.5",
        "progress step=3/2 phase=solve fraction=0.5",
    ] {
        if Event::parse(line) != Event::Log(line.into()) {
            bad.push(format!("{line}: {:?}", Event::parse(line)));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    for (line, step, steps, fraction) in [
        ("progress step=1/1 phase=solve fraction=0", 1, 1, 0.0),
        ("progress step=2/2 phase=solve fraction=1", 2, 2, 1.0),
    ] {
        assert_eq!(
            Event::parse(line),
            Event::Progress {
                step,
                steps,
                phase: "solve".into(),
                fraction
            }
        );
    }
}

#[test]
fn cancel_line_reaches_the_solver() {
    let dir = scratch("cancel");
    // The solver waits for a line on stdin and exits with code 130.
    let s = solver(
        &dir,
        r#"echo "fh-solver-protocol 2"
read line
[ "$line" = cancel ] && exit 130
exit 0"#,
    );
    let token = CancelToken::new();
    let t = token.clone();
    let h = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        t.cancel();
    });
    let e = s
        .run_with(&dir.join("a.inp"), &dir.join("run"), &token, &mut |_| {})
        .unwrap_err();
    h.join().unwrap();
    assert!(e.is_cancelled(), "{e}");
}

#[test]
fn a_cancel_is_not_a_success_when_the_solver_exits_0() {
    // The solver writes complete results, reads the cancel line and exits 0, not 130.
    let dir = scratch("cancel-exit-0");
    let s = solver(&dir, &format!("{GOOD}\nread line\nexit 0"));
    let token = CancelToken::new();
    let e = s
        .run_with(&dir.join("a.inp"), &dir.join("run"), &token, &mut |_| {
            token.cancel()
        })
        .unwrap_err();
    assert!(e.is_cancelled(), "{e}");
}

#[test]
fn solver_that_ignores_cancel_is_killed_after_the_grace_time() {
    let dir = scratch("stubborn");
    let mut s = solver(&dir, "echo \"fh-solver-protocol 2\"\nsleep 30");
    s.cancel_grace = Duration::from_millis(300);
    let token = CancelToken::new();
    token.cancel();
    let start = std::time::Instant::now();
    let e = s
        .run_with(&dir.join("a.inp"), &dir.join("run"), &token, &mut |_| {})
        .unwrap_err();
    assert!(e.is_cancelled(), "{e}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn time_limit_kills_the_whole_process_tree() {
    let dir = scratch("tree");
    // The grandchild runs in the group of the solver. Its pid goes to a file.
    let mut s = solver(
        &dir,
        &format!(
            "echo \"fh-solver-protocol 2\"\nsleep 30 &\necho $! > {}/grandchild.pid\nwait",
            dir.display()
        ),
    );
    s.timeout = Duration::from_millis(500);
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("time limit"), "{e}");
    let pid = fs::read_to_string(dir.join("grandchild.pid")).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(!alive(pid.trim()), "grandchild {pid} is still alive");
}

/// Kill the process group of `pid`, so that a failed test leaves no solver behind.
fn kill_group(pid: &str) {
    let _ = std::process::Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stderr(std::process::Stdio::null())
        .status();
}

#[test]
fn a_log_file_that_cannot_open_leaves_no_solver() {
    // stdout.txt is a folder, so the runner cannot open its log.
    let dir = scratch("log-error");
    let s = solver(&dir, "echo $$ > ../child.pid\nsleep 30");
    let run = dir.join("run");
    fs::create_dir_all(run.join("stdout.txt")).unwrap();
    fs::write(run.join(fh_solver_api::RUN_MARKER), "femahoy run folder\n").unwrap();
    let e = s
        .resume_with(&dir.join("a.inp"), &run, &CancelToken::new(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("stdout.txt"), "{e}");
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(pid) = fs::read_to_string(dir.join("child.pid")) {
        let running = alive(pid.trim());
        kill_group(pid.trim());
        assert!(!running, "solver {pid} runs on after the error");
    }
}

#[test]
fn a_panic_in_the_event_callback_leaves_no_solver() {
    let dir = scratch("panic");
    let s = solver(
        &dir,
        "echo \"fh-solver-protocol 2\"\necho $$ > ../child.pid\necho \"log x\"\nsleep 30",
    );
    let run = dir.join("run");
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.run_with(&dir.join("a.inp"), &run, &CancelToken::new(), &mut |_| {
            panic!("callback")
        })
    }));
    assert!(r.is_err());
    let pid = fs::read_to_string(dir.join("child.pid")).unwrap();
    let running = alive(pid.trim());
    kill_group(pid.trim());
    assert!(!running, "solver {pid} runs on after the panic");
}

#[test]
fn stale_files_do_not_pass_a_new_run() {
    let dir = scratch("stale");
    let s = solver(&dir, GOOD);
    s.run(&dir.join("a.inp"), &dir.join("run")).unwrap();
    // The second solver prints the header and exits at once.
    let s2 = solver(&dir, "echo \"fh-solver-protocol 2\"");
    let e = s2.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("summary.toon"), "{e}");
}

#[test]
fn stale_files_do_not_pass_a_restart() {
    let dir = scratch("stale-restart");
    let s = solver(&dir, GOOD);
    let run = dir.join("run");
    s.run(&dir.join("a.inp"), &run).unwrap();
    // The restart prints the header and exits at once, so it writes no new summary.
    let s2 = solver(&dir, "echo \"fh-solver-protocol 2\"");
    let e = s2
        .resume_with(&dir.join("a.inp"), &run, &CancelToken::new(), &mut |_| {})
        .unwrap_err();
    assert!(e.to_string().contains("summary.toon"), "{e}");
    // The restart state and the earlier results stay for the solver.
    assert!(run.join("restart.txt").is_file() && run.join("step-1.vtu").is_file());
}

#[test]
fn run_folder_with_foreign_files_is_not_cleared() {
    let dir = scratch("foreign");
    let s = solver(&dir, GOOD);
    let run = dir.join("run");
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("precious.txt"), "keep").unwrap();
    let e = s.run(&dir.join("a.inp"), &run).unwrap_err();
    assert!(e.to_string().contains("not a run folder"), "{e}");
    assert!(run.join("precious.txt").is_file());
}

#[test]
fn an_input_inside_the_run_folder_is_refused_and_kept() {
    let dir = scratch("input-inside");
    let s = solver(&dir, GOOD);
    let run = dir.join("run");
    s.run(&dir.join("a.inp"), &run).unwrap();
    fs::write(run.join("model.inp"), "*NODE\n").unwrap();
    let e = s.run(&run.join("model.inp"), &run).unwrap_err().to_string();
    assert!(e.contains("inside the run folder"), "{e}");
    assert!(run.join("model.inp").is_file());
}

#[test]
fn a_missing_input_keeps_the_earlier_results() {
    let dir = scratch("input-missing");
    let s = solver(&dir, GOOD);
    let run = dir.join("run");
    s.run(&dir.join("a.inp"), &run).unwrap();
    let e = s
        .run(&dir.join("missing.inp"), &run)
        .unwrap_err()
        .to_string();
    assert!(e.contains("input file"), "{e}");
    assert!(run.join("results.pvd").is_file());
}

#[test]
fn a_missing_solver_keeps_the_earlier_results() {
    let dir = scratch("solver-missing");
    let run = dir.join("run");
    solver(&dir, GOOD).run(&dir.join("a.inp"), &run).unwrap();
    let e = SubprocessSolver::new(dir.join("missing.sh"))
        .run(&dir.join("a.inp"), &run)
        .unwrap_err()
        .to_string();
    assert!(e.contains("cannot start"), "{e}");
    assert!(run.join("results.pvd").is_file());
}

#[test]
fn pvd_path_outside_the_run_folder_is_rejected() {
    for bad in ["../outside.vtu", "/etc/hosts"] {
        let dir = scratch("escape");
        fs::write(dir.join("outside.vtu"), "x").unwrap();
        let body = GOOD.replace("file=\"step-1.vtu\"", &format!("file=\"{bad}\""));
        let e = run_in(&dir, &body).unwrap_err();
        assert!(e.contains("not a path inside"), "{bad}: {e}");
    }
}

/// `GOOD` with this PVD text in place of its own.
fn with_pvd(pvd: &str) -> String {
    let own = GOOD.lines().find(|l| l.contains("results.pvd")).unwrap();
    GOOD.replace(own, &format!("cat > results.pvd <<'EOF'\n{pvd}\nEOF"))
}

#[test]
fn a_valid_pvd_with_single_quotes_and_comments_passes() {
    let pvd = "<?xml version='1.0'?>\n<!-- not an entry: file=\"x.vtu\" -->\n\
<VTKFile type='Collection' version='0.1'>\n<Collection units='m &amp; s'>\n\
<DataSet timestep='1' part='0' file='step-1.vtu'/>\n</Collection>\n</VTKFile>";
    let r = run("pvd-quotes", &with_pvd(pvd));
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn a_pvd_that_is_not_a_collection_is_rejected() {
    let entry = "<DataSet timestep=\"1\" part=\"0\" file=\"step-1.vtu\"/>";
    for pvd in [
        "garbage file=\"step-1.vtu\"".to_string(),
        format!("<VTKFile type=\"Collection\"><Collection>{entry}</Collection>"),
        format!("<VTKFile type=\"UnstructuredGrid\"><Collection>{entry}</Collection></VTKFile>"),
        format!("<VTKFile type=\"Collection\"><Collection>{entry}</VTKFile></Collection>"),
        format!("<VTKFile type=\"Collection\"><Collection>x {entry}</Collection></VTKFile>"),
        "<VTKFile type=\"Collection\"><Collection><DataSet file=\"step-1.vtu></Collection></VTKFile>"
            .to_string(),
    ] {
        let e = run("pvd-bad", &with_pvd(&pvd)).unwrap_err();
        assert!(e.contains("results.pvd is not a PVD collection"), "{pvd}: {e}");
    }
}

fn run_in(dir: &Path, body: &str) -> Result<RunReport, String> {
    let exe = script(dir, body);
    SubprocessSolver::new(exe)
        .run(&dir.join("a.inp"), &dir.join("run"))
        .map_err(|e| e.to_string())
}

#[test]
fn summary_must_be_an_fh_run_document() {
    let e = run(
        "badsummary",
        &GOOD.replace("schema: fh.run", "status: fine"),
    )
    .unwrap_err();
    assert!(e.contains("fh.run"), "{e}");
}

#[test]
fn unavailable_memory_probe_stops_the_solver() {
    let dir = scratch("noprobe");
    let mut s = solver(&dir, "echo \"fh-solver-protocol 2\"\nsleep 30");
    s.probe = |_| Err("no probe here".into());
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("memory probe"), "{e}");
}

/// A solver that writes the header and then log lines without end.
const FLOOD: &str = "echo \"fh-solver-protocol 2\"\nwhile true; do echo \"log x\"; done";

/// Run `s` on `FLOOD` in a thread, with an event callback of 2 ms that can also cancel.
/// The runner must return within 5 s. Its error and whether it is a cancel.
fn run_flood(s: SubprocessSolver, dir: &Path, cancel: bool) -> (bool, String) {
    let (inp, run) = (dir.join("a.inp"), dir.join("run"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let token = CancelToken::new();
        let r = s.run_with(&inp, &run, &token, &mut |_| {
            if cancel {
                token.cancel();
            }
            std::thread::sleep(Duration::from_millis(2));
        });
        let _ = tx.send(r.map_err(|e| (e.is_cancelled(), e.to_string())));
    });
    let r = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("the runner did not return within 5 s");
    r.unwrap_err()
}

#[test]
fn a_stream_of_events_cannot_delay_the_time_limit() {
    let dir = scratch("flood-time");
    let mut s = solver(&dir, FLOOD);
    s.timeout = Duration::from_millis(500);
    let (_, e) = run_flood(s, &dir, false);
    assert!(e.contains("time limit"), "{e}");
}

fn over_the_limit(_: u32) -> Result<u64, String> {
    Ok(100 << 20)
}

#[test]
fn a_stream_of_events_cannot_delay_the_memory_check() {
    let dir = scratch("flood-memory");
    let mut s = solver(&dir, FLOOD);
    s.memory_limit = 50 << 20;
    s.probe = over_the_limit;
    let (_, e) = run_flood(s, &dir, false);
    assert!(e.contains("memory limit"), "{e}");
}

#[test]
fn a_stream_of_events_cannot_delay_the_cancel() {
    // The solver does not read stdin, so the runner kills it after the grace time.
    let dir = scratch("flood-cancel");
    let mut s = solver(&dir, FLOOD);
    s.cancel_grace = Duration::from_millis(200);
    let (cancelled, e) = run_flood(s, &dir, true);
    assert!(cancelled, "{e}");
}

#[test]
fn a_long_stdout_line_is_refused_and_stops_the_solver() {
    // 16 MiB without a newline. The runner must not keep all of it in memory.
    let dir = scratch("long-line");
    let mut s = solver(
        &dir,
        "echo \"fh-solver-protocol 2\"\nhead -c 16777216 /dev/zero\nsleep 30",
    );
    s.timeout = Duration::from_secs(3);
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("stdout line longer than"), "{e}");
}

#[test]
fn a_slow_callback_keeps_only_a_small_backlog_of_lines() {
    // While the callback waits, the reader must stop at a bounded number of lines, and
    // not copy all the output of the solver into memory. The log shows how far it read.
    let dir = scratch("backlog");
    let s = solver(
        &dir,
        "echo \"fh-solver-protocol 2\"\nyes \"log x\" | head -n 200000",
    );
    let run = dir.join("run");
    let mut read = None;
    let _ = s.run_with(&dir.join("a.inp"), &run, &CancelToken::new(), &mut |_| {
        if read.is_none() {
            std::thread::sleep(Duration::from_millis(500));
            read = Some(
                fs::read_to_string(run.join("stdout.txt"))
                    .unwrap()
                    .lines()
                    .count(),
            );
        }
    });
    let read = read.expect("no event");
    assert!(
        read < 5000,
        "the runner read {read} lines before the callback took the first"
    );
}

#[test]
fn a_large_summary_or_pvd_file_is_refused() {
    // 32 MiB of zero bytes after a valid file. The runner must not read all of it.
    for name in ["summary.toon", "results.pvd"] {
        let body = format!("{GOOD}\nhead -c 33554432 /dev/zero >> {name}");
        let e = match run(&format!("large-{name}"), &body) {
            Ok(_) => panic!("a {name} of 32 MiB passed"),
            Err(e) => e,
        };
        assert!(e.contains(&format!("{name} is larger than")), "{e}");
    }
}

#[test]
fn a_large_stderr_shows_only_its_end_in_the_error() {
    let e = run(
        "large-stderr",
        "head -c 33554432 /dev/zero >&2; echo 'bad mesh' >&2; exit 3",
    )
    .unwrap_err();
    assert!(
        e.contains("bad mesh"),
        "{}",
        &e[e.len().saturating_sub(200)..]
    );
    assert!(e.len() < 1 << 20, "the error has {} bytes", e.len());
}

#[test]
fn estimate_above_the_limit_stops_the_solver_early() {
    let dir = scratch("estimate");
    let mut s = solver(
        &dir,
        "echo \"fh-solver-protocol 2\"\necho \"estimate bytes=9000000000\"\nsleep 30",
    );
    s.memory_limit = 1 << 30;
    let e = s.run(&dir.join("a.inp"), &dir.join("run")).unwrap_err();
    assert!(e.to_string().contains("expects to need"), "{e}");
}

#[test]
fn default_memory_limit_follows_the_machine_ram() {
    let limit = fh_solver_api::default_memory_limit();
    match fh_solver_api::total_memory() {
        Some(ram) => assert!(limit > ram / 2 && limit < ram, "{limit} of {ram}"),
        None => assert_eq!(limit, u64::MAX),
    }
}

struct Fake;

impl Solver for Fake {
    fn name(&self) -> &str {
        "fake"
    }
    fn validated(&self) -> bool {
        false
    }
    fn solve(&self, _: &fh_inp::Model, _: &Path) -> Result<RunReport, fh_solver_api::ApiError> {
        Err(fh_solver_api::ApiError("no".into()))
    }
}

#[test]
fn registry_keeps_solvers_by_name() {
    let mut r = Registry::new();
    r.register(Box::new(Fake)).unwrap();
    assert!(r.register(Box::new(Fake)).is_err());
    assert_eq!(r.names(), vec!["fake"]);
    assert!(r.get("fake").is_some() && r.get("other").is_none());
}
