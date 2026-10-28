//! `run_case_with` with a solver that gives an error: the case fails, and the reason names the
//! error.

use std::path::{Path, PathBuf};

use fh::validation::{run_case_with, Case, Outcome, Solve};

/// The shipped bar case, with one reference.
fn case(reference: f64, rel_tol: f64) -> Case {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases/bar-pressure");
    let mut c = Case::load(&dir).unwrap();
    c.references.truncate(1);
    c.references[0].value = reference;
    (c.rel_tol, c.abs_tol) = (rel_tol, 0.0);
    c
}

#[test]
fn a_solver_error_is_a_case_that_cannot_run() {
    struct Broken;
    impl Solve for Broken {
        fn measure(&self, _: &Case, _: &Path) -> Result<Vec<f64>, String> {
            Err("no result".into())
        }
    }
    let r = run_case_with(&case(1.0, 0.1), &Broken);
    let Outcome::Fail(why) = &r.outcome else {
        panic!("{r:?}")
    };
    assert!(
        why.contains("cannot run") && why.contains("no result"),
        "{why}"
    );
}

/// The cases of the built-in grid and line solvers need no solver program, so they run in each
/// test run.
#[test]
fn the_grid_cases_pass() {
    let all = fh::validation::load_all(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases"),
    )
    .unwrap();
    let grid: Vec<&Case> = all
        .iter()
        .filter(|c| c.id.starts_with("grid-") || c.id.starts_with("line-"))
        .collect();
    assert!(!grid.is_empty());
    for case in grid {
        let r = run_case_with(case, &fh::validation::Subprocess::default());
        assert!(matches!(r.outcome, Outcome::Pass), "{}: {r:?}", case.id);
    }
}

/// The native solve of `fh-quine` passes a grid case and keeps its results in the run folder. It
/// refuses a CalculiX case, and the reason names CalculiX.
#[test]
fn the_native_solve_keeps_the_results_and_refuses_calculix() {
    let cases = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases");
    let run = std::env::temp_dir().join(format!("fh-native-{}", std::process::id()));
    let native = fh::validation::Native { run: run.clone() };
    let cell = Case::load(&cases.join("grid-laminate-cell")).unwrap();
    let r = run_case_with(&cell, &native);
    assert!(matches!(r.outcome, Outcome::Pass), "{r:?}");
    assert!(run.join("cell/grid/homogenized.txt").is_file());
    let _ = std::fs::remove_dir_all(&run);

    let r = run_case_with(&case(1.0, 0.1), &native);
    let Outcome::Fail(why) = &r.outcome else {
        panic!("{r:?}")
    };
    assert!(why.contains("CalculiX"), "{why}");
}
