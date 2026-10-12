//! The `fh-solve` executable under protocol v2, through the subprocess runner.

use std::fs;
use std::path::PathBuf;

use fh_core_solver::solve;
use fh_io::vtu::{read_pvd, read_vtu};
use fh_solver_api::{CancelToken, Event, SubprocessSolver};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}/../../fixtures/inp/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn run_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fh-protocol-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn cantilever_runs_through_protocol_v2() {
    let inp = fixture("cantilever.inp");
    let dir = run_dir("cantilever");
    let solver = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"));
    let report = solver.run(&inp, &dir).unwrap();
    assert!(
        report.summary.starts_with("schema: fh.run\n"),
        "{}",
        report.summary
    );
    assert!(report.summary.contains("status: ok"), "{}", report.summary);
    assert!(report.restart.is_some());

    // The VTU displacement equals the in-process solution.
    let model = fh_inp::parse(&fs::read_to_string(&inp).unwrap()).unwrap();
    let want = solve(&model).unwrap();
    let entries = read_pvd(&fs::read_to_string(&report.pvd).unwrap()).unwrap();
    assert_eq!(entries.len(), want.steps.len());
    let grid = read_vtu(&fs::read(dir.join(&entries[0].file)).unwrap()).unwrap();
    let u = grid.point_field("U").unwrap().as_f64().unwrap();
    let expected: Vec<f64> = want.steps[0]
        .displacement
        .iter()
        .flatten()
        .copied()
        .collect();
    assert_eq!(u, expected.as_slice());
    assert_eq!(grid.cells.len(), model.elements.len());
    assert!(grid.cell_field("MISES").is_some());
    assert!(grid.point_field("NSET_FIXED").is_some());
}

#[test]
fn bad_input_gives_a_failure_with_the_reason() {
    let dir = run_dir("bad");
    fs::create_dir_all(&dir).unwrap();
    let inp = dir.join("bad.inp");
    fs::write(&inp, "*PLASTIC\n").unwrap();
    let e = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"))
        .run(&inp, &dir.join("run"))
        .unwrap_err();
    assert!(e.to_string().contains("solver failed"), "{e}");
}

fn two_step_input(dir: &std::path::Path) -> PathBuf {
    fs::create_dir_all(dir).unwrap();
    let text = fs::read_to_string(fixture("cantilever.inp")).unwrap();
    let inp = dir.join("two.inp");
    fs::write(&inp, text).unwrap();
    inp
}

#[test]
fn events_report_the_estimate_progress_and_restart_marker() {
    let dir = run_dir("events");
    let inp = two_step_input(&dir);
    let mut seen = Vec::new();
    SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"))
        .run_with(&inp, &dir.join("run"), &CancelToken::new(), &mut |e| {
            seen.push(e.clone())
        })
        .unwrap();
    assert!(
        matches!(seen.first(), Some(Event::Estimate { bytes }) if *bytes > 0),
        "{seen:?}"
    );
    assert!(seen
        .iter()
        .any(|e| matches!(e, Event::Progress { phase, .. } if phase == "solve")));
    assert!(seen.contains(&Event::Restart("restart.fh".into())));
    let restart = fs::read_to_string(dir.join("run/restart.fh")).unwrap();
    assert!(
        restart.starts_with("fh-restart 1\ninput-hash "),
        "{restart}"
    );
}

#[test]
fn a_restart_skips_the_finished_steps() {
    let dir = run_dir("restart");
    fs::create_dir_all(&dir).unwrap();
    let inp = dir.join("two.inp");
    let text = fs::read_to_string(fixture("cantilever.inp")).unwrap();
    let two = format!("{text}\n*STEP, NAME=SECOND\n*STATIC\n*END STEP\n");
    fs::write(&inp, &two).unwrap();
    let solver = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"));
    let report = solver.run(&inp, &dir.join("run")).unwrap();
    let first = fs::read(dir.join("run/step-1.vtu")).unwrap();
    // Cut the restart file back to one finished step, then resume.
    let restart = fs::read_to_string(dir.join("run/restart.fh")).unwrap();
    let kept: Vec<&str> = restart
        .lines()
        .filter(|l| !l.starts_with("step ") || l.is_empty())
        .collect();
    let one_line = restart.lines().find(|l| l.starts_with("step ")).unwrap();
    let cut = kept
        .iter()
        .map(|l| {
            if l.starts_with("steps-done") {
                "steps-done 1"
            } else {
                l
            }
        })
        .chain(std::iter::once(one_line))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(dir.join("run/restart.fh"), format!("{cut}\n")).unwrap();
    fs::remove_file(dir.join("run/step-2.vtu")).ok();
    let again = solver
        .resume_with(&inp, &dir.join("run"), &CancelToken::new(), &mut |_| {})
        .unwrap();
    assert_eq!(fs::read(dir.join("run/step-1.vtu")).unwrap(), first);
    assert!(dir.join("run/step-2.vtu").is_file());
    assert_eq!(
        read_pvd(&fs::read_to_string(&again.pvd).unwrap())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        report.summary.lines().count(),
        again.summary.lines().count()
    );
}

#[test]
fn a_restart_with_a_changed_input_is_refused() {
    let dir = run_dir("restart-changed");
    let inp = two_step_input(&dir);
    let solver = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"));
    solver.run(&inp, &dir.join("run")).unwrap();
    let text = fs::read_to_string(&inp).unwrap();
    fs::write(&inp, format!("** changed\n{text}")).unwrap();
    let e = solver
        .resume_with(&inp, &dir.join("run"), &CancelToken::new(), &mut |_| {})
        .unwrap_err();
    assert!(
        e.to_string().contains("not the one of the earlier run"),
        "{e}"
    );
}

#[test]
fn a_cancel_line_stops_the_solver() {
    let dir = run_dir("cancel");
    let inp = two_step_input(&dir);
    let token = CancelToken::new();
    token.cancel();
    let e = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"))
        .run_with(&inp, &dir.join("run"), &token, &mut |_| {})
        .unwrap_err();
    assert!(e.is_cancelled(), "{e}");
}
