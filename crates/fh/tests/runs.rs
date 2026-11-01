//! `fh runs`: the table of the runs and the folder and the report of 1 run. A fake `ccx` that
//! waits gives a run that runs, a run that `fh run` stops with SIGKILL and a run that the time
//! limit fails. The grid case gives a run that succeeds with no solver program.
mod common;
use common::{p, root, scratch, script, stderr, stdout};

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const WAITING_CCX: &str = "#!/bin/sh\nsleep 300 &\nwait\n";

fn fh(state: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fh"));
    command.args(args).env("FH_STATE", state);
    command
}

/// The table of `fh runs`.
fn table(state: &Path) -> String {
    let out = fh(state, &["runs"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    stdout(&out)
}

/// The row of `run` in the table of `fh runs`.
fn row_of(state: &Path, run: &str) -> String {
    let table = table(state);
    let row = table.lines().find(|l| l.starts_with(run));
    row.unwrap_or_else(|| panic!("no {run}: {table}"))
        .to_owned()
}

fn wait_for_row(state: &Path, run: &str, status: &str) -> String {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let table = table(state);
        let row = table.lines().find(|l| l.starts_with(run)).unwrap_or("");
        if row.split_whitespace().nth(2) == Some(status) {
            return row.to_owned();
        }
        assert!(Instant::now() < end, "timeout: {table}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn runs(state: &Path, run: &str) -> Output {
    fh(state, &["runs", run]).output().unwrap()
}

#[test]
fn fh_runs_shows_the_status_the_time_and_the_key_result_of_each_run() {
    let dir = scratch("runs");
    let state = dir.join("state");
    let ccx = script(&dir.join("ccx"), WAITING_CCX);
    let model = root()
        .join("validation/cases/bar-pressure/job.inp")
        .canonicalize()
        .unwrap();

    assert!(table(&state).starts_with("no runs in "));

    // A run that runs, then a kill of `fh run`.
    let mut child = fh(&state, &["run", p(&model)])
        .env("FH_CCX", &ccx)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let running = wait_for_row(&state, "run-000001", "running");
    assert!(
        running.contains(" ccx ") && running.ends_with(p(&model)),
        "{running}"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    wait_for_row(&state, "run-000001", "killed");

    let failed = fh(&state, &["run", p(&model), "--timeout", "1"])
        .env("FH_CCX", &ccx)
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1), "{}", stderr(&failed));
    let row = row_of(&state, "run-000002");
    assert!(
        row.contains(" failed ") && row.contains("time limit"),
        "{row}"
    );

    let wall = root().join("validation/cases/grid-layered-wall/wall");
    let solved = fh(&state, &["run", p(&wall)]).output().unwrap();
    assert_eq!(solved.status.code(), Some(0), "{}", stderr(&solved));
    let row = row_of(&state, "run-000003");
    let cells: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(cells[..3], ["run-000003", "grid", "succeeded"], "{row}");
    assert!(row.contains(" s  1 step  "), "{row}");

    let folder = state.join("runs/run-000003").canonicalize().unwrap();
    let shown = stdout(&runs(&state, "3"));
    let folder_line = format!("folder {}\n", state.join("runs/run-000003").display());
    assert_eq!(shown, format!("{folder_line}report none\n"));
    fs::write(folder.join("report.html"), "<p>report</p>").unwrap();
    let report = state.join("runs/run-000003/report.html");
    assert_eq!(
        stdout(&runs(&state, "run-000003")),
        format!("{folder_line}report {}\n", report.display())
    );

    let missing = runs(&state, "9");
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        stderr(&missing).contains("no run run-000009"),
        "{}",
        stderr(&missing)
    );
    let _ = fs::remove_dir_all(&dir);
}
