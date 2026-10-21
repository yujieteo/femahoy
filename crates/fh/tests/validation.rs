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
