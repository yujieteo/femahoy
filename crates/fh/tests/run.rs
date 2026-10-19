//! `fh run`: one solve with the real ccx on the input of a validation case, and the stop of
//! ccx when `fh run` ends for any reason. The stop tests use a small fake `ccx` that waits.
mod common;
use common::{ccx_solver, p, root, scratch, script, stderr, stdout};

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A fake ccx. It writes its pid and the pid of its child, then waits. It ignores SIGTERM, so
/// only a kill of its process group stops it.
const WAITING_CCX: &str = "#!/bin/sh\ntrap '' TERM\nsleep 300 &\n\
    echo $$ $! > PIDS.tmp\nmv PIDS.tmp PIDS\nwait\n";

struct Lab {
    dir: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = scratch(name);
        script(
            &dir.join("ccx"),
            &WAITING_CCX.replace("PIDS", p(&dir.join("pids"))),
        );
        Lab { dir }
    }

    /// `fh run MODEL ARGS` with the fake ccx. It ends with the stop of the test.
    fn start(&self, args: &[&str]) -> Child {
        let model = root().join("validation/cases/bar-pressure/job.inp");
        Command::new(env!("CARGO_BIN_EXE_fh"))
            .args(["run", p(&model)])
            .args(args)
            .env("FH_STATE", self.dir.join("state"))
            .env("FH_CCX", self.dir.join("ccx"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start fh run")
    }

    /// The pids of the fake ccx, when it runs.
    fn pids(&self) -> Vec<String> {
        wait_for("the fake ccx", || {
            let text = fs::read_to_string(self.dir.join("pids")).ok()?;
            Some(text.split_whitespace().map(str::to_owned).collect())
        })
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < end, "timeout: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn signal(pid: u32, name: &str) {
    let status = Command::new("kill").arg(name).arg(pid.to_string()).status();
    assert!(status.unwrap().success());
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Each pid is gone: the fake ccx and its child.
#[track_caller]
fn assert_stopped(pids: &[String]) {
    assert_eq!(pids.len(), 2, "{pids:?}");
    wait_for(&format!("the stop of {pids:?}"), || {
        (!pids.iter().any(|p| alive(p))).then_some(())
    });
}

/// Send `name` to `fh run` after ccx started. `fh run` must end by itself and stop ccx.
fn stop_with(name: &str, name_of_test: &str) -> (std::process::Output, Vec<String>) {
    let lab = Lab::new(name_of_test);
    let child = lab.start(&[]);
    let pids = lab.pids();
    assert!(pids.iter().all(|p| alive(p)), "ccx runs before the signal");
    signal(child.id(), name);
    let out = child.wait_with_output().unwrap();
    assert_stopped(&pids);
    (out, pids)
}

#[test]
fn ctrl_c_stops_ccx() {
    let (out, _) = stop_with("-INT", "run-int");
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
}

#[test]
fn sigterm_stops_ccx() {
    let (out, _) = stop_with("-TERM", "run-term");
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
}

/// SIGKILL gives `fh run` no time. The guard process of the solver stops ccx.
#[test]
fn a_kill_of_fh_run_stops_ccx() {
    let (out, _) = stop_with("-KILL", "run-kill");
    assert_eq!(out.status.code(), None, "ended by the signal");
}

#[test]
fn the_time_limit_stops_ccx_and_fails_the_run() {
    let lab = Lab::new("run-timeout");
    let out = lab.start(&["--timeout", "3"]).wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("time limit"), "{}", stderr(&out));
    assert_stopped(&lab.pids());
}

/// The mean of the first values of `S` is the stress along the bar.
fn mean_of_first(values: &[f64], components: usize) -> f64 {
    let first: Vec<f64> = values.iter().step_by(components).copied().collect();
    first.iter().sum::<f64>() / first.len() as f64
}

/// The real ccx solves the input of `bar-pressure` with a request for the fields of the page.
/// The result files hold the known solution: 100 MPa along the bar and a tip of 0.047619 mm.
#[test]
fn fh_run_solves_a_validation_input_and_writes_the_result_files() {
    let Some(solver) = ccx_solver() else { return };
    let dir = scratch("run-ccx");
    let case = fs::read_to_string(root().join("validation/cases/bar-pressure/job.inp")).unwrap();
    let input = dir.join("job.inp");
    let text = case.replace("*END STEP", "*NODE FILE\nU\n*EL FILE\nS\n*END STEP");
    assert_ne!(text, case);
    fs::write(&input, &text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(&input)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_CCX", &solver)
        .output()
        .unwrap();
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");

    let run = dir.join("state/runs/run-000001");
    let results = run.join("out");
    let paths = format!(
        "fh run: text results {0}/ccx/job.dat (print requests), {0}/ccx/job.sta (steps)",
        results.display()
    );
    assert_eq!(shown.lines().last(), Some(paths.as_str()), "{shown}");
    for file in ["ccx/job.dat", "ccx/job.sta", "results.pvd", "step-1.vtu"] {
        assert!(results.join(file).is_file(), "no {file}: {shown}");
    }
    assert_eq!(fs::read_to_string(run.join("in/input.inp")).unwrap(), text);
    assert_eq!(
        fs::read_to_string(run.join("source")).unwrap(),
        fs::canonicalize(&input).unwrap().display().to_string()
    );
    assert!(dir.join("state/previews/1/input.inp").is_file());
    let pvd = fs::read_to_string(results.join("results.pvd")).unwrap();
    assert!(pvd.contains("step-1.vtu") && pvd.contains("SI-mm"), "{pvd}");

    let grid = fh_core::vtu::read_vtu(&fs::read(results.join("step-1.vtu")).unwrap()).unwrap();
    let field = |name: &str| grid.point_field(name).unwrap().as_f64().unwrap().to_vec();
    let stress = mean_of_first(&field("S"), 6);
    assert!((stress - 100.0).abs() < 1e-3, "S xx {stress}");
    let tip = field("U").iter().step_by(3).fold(0.0_f64, |m, v| m.max(*v));
    let want = 100.0 / 210000.0 * 100.0;
    assert!((tip - want).abs() < 1e-5 * want, "tip {tip} {want}");
    let _ = fs::remove_dir_all(&dir);
}
