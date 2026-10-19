//! The pass rule of `run_case_with`. A case passes only when its margin is finite and not
//! negative. When the tolerance and the difference both overflow, the margin is NaN, and a test
//! for "margin below 0" lets the case pass.

use std::path::{Path, PathBuf};

use fh_validation::{run_case_with, Case, Outcome, Solve};

/// A solver that gives one fixed value for each reference.
struct Value(f64);

impl Solve for Value {
    fn measure(&self, case: &Case, _: &Path) -> Result<Vec<f64>, String> {
        Ok(vec![self.0; case.references.len()])
    }
}

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
fn a_value_inside_the_tolerance_passes_and_outside_fails() {
    let pass = run_case_with(&case(-1.0, 0.1), &Value(-1.05));
    assert_eq!(pass.outcome, Outcome::Pass, "{pass:?}");
    assert!(pass.rows[0].margin > 0.0, "{pass:?}");
    let fail = run_case_with(&case(-1.0, 0.1), &Value(-1.2));
    assert!(matches!(fail.outcome, Outcome::Fail(_)), "{fail:?}");
    assert!(fail.rows[0].margin < 0.0, "{fail:?}");
}

#[test]
fn an_overflow_of_tolerance_and_difference_fails() {
    // Value -1.7e308, reference 1.7e308: the difference 3.4e308 and the tolerance
    // 1.1 * 1.7e308 = 1.87e308 both overflow.
    let r = run_case_with(&case(1.7e308, 1.1), &Value(-1.7e308));
    assert!(matches!(r.outcome, Outcome::Fail(_)), "{r:?}");
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
