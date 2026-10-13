//! The pass rule of `run_case_with` for class L. A case passes only when its margin is finite
//! and not negative. When the tolerance and the difference both overflow, the margin is NaN,
//! and a test for "margin below 0" lets the case pass.

use std::path::{Path, PathBuf};

use fh_validation::{run_case_with, Case, Class, Outcome, Solve};

/// A solver that gives a fixed value for each mesh, counted from the finest mesh.
struct Values(Vec<f64>);

impl Solve for Values {
    fn measure(&self, case: &Case, mesh: &Path) -> Result<Vec<f64>, String> {
        let i = case.meshes.iter().position(|m| m == mesh).unwrap();
        let from_finest = case.meshes.len() - 1 - i;
        let v = self.0.get(from_finest).copied().unwrap_or(0.0);
        Ok(vec![v; case.references.len()])
    }
}

/// The shipped cantilever case (class L), with one reference.
fn case(reference: f64, reference_error: f64) -> Case {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases/cantilever");
    let mut c = Case::load(&dir).unwrap();
    c.class = Class::L {
        ratio: 2.0,
        reference_error,
        order_min: 0.5,
        order_max: 2.0,
    };
    c.references.truncate(1);
    c.references[0].value = reference;
    c
}

/// Finest first: the differences are 1 and 2 units, so the order is 1, the limit is the
/// finest value less 1 unit, and the estimate is 1 unit.
fn values(finest: f64, unit: f64) -> Values {
    Values(vec![finest, finest + unit, finest + 3.0 * unit])
}

#[test]
fn a_limit_inside_the_tolerance_passes_and_outside_fails() {
    let pass = run_case_with(&case(-1.0, 0.0), &values(0.0, 1.0));
    assert_eq!(pass.outcome, Outcome::Pass, "{pass:?}");
    let fail = run_case_with(&case(-3.0, 0.0), &values(0.0, 1.0));
    assert!(matches!(fail.outcome, Outcome::Fail(_)), "{fail:?}");
}

#[test]
fn an_overflow_of_tolerance_and_difference_fails() {
    // Limit -1.7e308 - 1e300, reference 1.7e308: the difference 3.4e308 and the tolerance
    // 1e300 + 1.1 * 1.7e308 = 1.87e308 both overflow, and the difference is the larger one.
    let r = run_case_with(&case(1.7e308, 1.1), &values(-1.7e308, 1e300));
    let row = &r.rows[0];
    assert!((row.computed - (-1.7e308 - 1e300)).abs() < 1e295, "{r:?}");
    assert!(matches!(r.outcome, Outcome::Fail(_)), "{r:?}");
}

/// A solver for class R that gives the error `c * h^p` for each mesh.
struct Power {
    c: f64,
    p: f64,
}

impl Solve for Power {
    fn measure(&self, _: &Case, _: &Path) -> Result<Vec<f64>, String> {
        Ok(vec![])
    }

    fn field_error(&self, case: &Case, mesh: &Path) -> Result<f64, String> {
        let i = case.meshes.iter().position(|m| m == mesh).unwrap();
        Ok(self.c * case.manufactured.as_ref().unwrap().mesh_sizes[i].powf(self.p))
    }
}

/// The shipped `C3D10` class R case.
fn case_r() -> Case {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases/mms-c3d10");
    Case::load(&dir).unwrap()
}

#[test]
fn class_r_passes_inside_the_band_and_fails_outside() {
    let c = case_r();
    let Class::R { rate, band } = c.class else {
        panic!("not class R")
    };
    let pass = run_case_with(&c, &Power { c: 0.5, p: rate });
    assert_eq!(pass.outcome, Outcome::Pass, "{pass:?}");
    let row = &pass.rows[0];
    assert_eq!(row.name, "rate");
    assert!(
        (row.computed - rate).abs() < 1e-12 && row.margin > 0.0,
        "{row:?}"
    );
    for p in [rate - band - 0.1, rate + band + 0.1] {
        let r = run_case_with(&c, &Power { c: 0.5, p });
        assert!(matches!(r.outcome, Outcome::Fail(_)), "{p}: {r:?}");
        assert!(r.rows[0].margin < 0.0, "{p}: {r:?}");
    }
}

#[test]
fn class_r_fails_when_the_error_does_not_fall() {
    // A negative power makes the error grow as the mesh gets finer.
    let r = run_case_with(&case_r(), &Power { c: 0.5, p: -3.0 });
    let Outcome::Fail(why) = &r.outcome else {
        panic!("{r:?}")
    };
    assert!(why.contains("does not fall"), "{why}");
}

#[test]
fn class_r_cannot_run_without_a_field_error() {
    // `Values` measures point values only, so it keeps the default `field_error`.
    let r = run_case_with(&case_r(), &Values(vec![]));
    let Outcome::Fail(why) = &r.outcome else {
        panic!("{r:?}")
    };
    assert!(
        why.contains("cannot run") && why.contains("error of a field"),
        "{why}"
    );
    let mut c = case_r();
    c.manufactured = None;
    let r = run_case_with(&c, &Power { c: 1.0, p: 3.0 });
    assert!(matches!(r.outcome, Outcome::Fail(_)), "{r:?}");
}
