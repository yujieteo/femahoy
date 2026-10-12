//! The `fh-solve` executable under protocol v1, through the subprocess runner.

use std::fs;
use std::path::PathBuf;

use fh_core_solver::solve;
use fh_io::vtu::{read_pvd, read_vtu};
use fh_solver_api::SubprocessSolver;
use fh_toon::Value;

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
fn cantilever_runs_through_protocol_v1() {
    let inp = fixture("cantilever.inp");
    let dir = run_dir("cantilever");
    let solver = SubprocessSolver::new(env!("CARGO_BIN_EXE_fh-solve"));
    let report = solver.run(&inp, &dir).unwrap();
    assert!(!report.validated);
    assert_eq!(report.summary.get("status"), Some(&Value::Str("ok".into())));

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
