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
fn pvd_path_outside_the_run_folder_is_rejected() {
    for bad in ["../outside.vtu", "/etc/hosts"] {
        let dir = scratch("escape");
        fs::write(dir.join("outside.vtu"), "x").unwrap();
        let body = GOOD.replace("file=\"step-1.vtu\"", &format!("file=\"{bad}\""));
        let e = run_in(&dir, &body).unwrap_err();
        assert!(e.contains("not a path inside"), "{bad}: {e}");
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
