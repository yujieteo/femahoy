//! Protocol v1 runner tests with small shell scripts as fake solvers.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use fh_solver_api::SubprocessSolver;

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

const GOOD: &str = r#"echo "fh-solver-protocol 1"
echo '<VTKFile type="Collection"><Collection><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>' > results.pvd
echo "<VTKFile/>" > step-1.vtu
printf 'status: ok\nsteps: 1\n' > summary.toon"#;

fn run(name: &str, body: &str) -> Result<fh_solver_api::RunReport, String> {
    let dir = scratch(name);
    let exe = script(&dir, body);
    SubprocessSolver::new(exe)
        .run(&dir.join("a.inp"), &dir.join("run"))
        .map_err(|e| e.to_string())
}

#[test]
fn good_solver_passes_and_is_unvalidated() {
    let r = run("good", GOOD).unwrap();
    assert!(!r.validated);
    assert!(r.pvd.ends_with("results.pvd"));
    assert_eq!(
        r.summary.get("status"),
        Some(&fh_toon::Value::Str("ok".into()))
    );
}

#[test]
fn missing_header_is_rejected() {
    let e = run(
        "nohead",
        &GOOD.replace("echo \"fh-solver-protocol 1\"", "echo hello"),
    )
    .unwrap_err();
    assert!(e.contains("does not speak protocol v1"), "{e}");
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
