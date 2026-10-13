//! Run a case with a solver and compare the numbers with the references.

use std::path::Path;

use crate::{check_n, check_r, observed_rate, richardson, Case, Class};

/// A solver that measures the quantities of a case on one mesh.
pub trait Solve {
    /// The value of each reference of `case`, in the order of `case.references`.
    fn measure(&self, case: &Case, mesh: &Path) -> Result<Vec<f64>, String>;

    /// Class R. The root mean square, over all nodes, of the difference between the computed
    /// displacement and `case.manufactured.exact`. The boundary values come from the exact field.
    fn field_error(&self, _case: &Case, _mesh: &Path) -> Result<f64, String> {
        Err("this solver cannot measure the error of a field".into())
    }
}

/// The result of one case.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// No solver was given, so the case cannot run.
    NoSolver,
    Pass,
    Fail(String),
}

/// One compared number.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    /// Class N: the value of the mesh. Class L: the extrapolated limit.
    pub computed: f64,
    pub reference: f64,
    /// The largest difference that passes.
    pub tolerance: f64,
    /// `tolerance - |computed - reference|`. Negative means a failure.
    pub margin: f64,
    /// Class L: the value of the finest mesh and the observed order.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    pub rows: Vec<Row>,
}

fn fail(why: String, rows: Vec<Row>) -> Report {
    Report {
        outcome: Outcome::Fail(why),
        rows,
    }
}

fn row(name: &str, computed: f64, reference: f64, tolerance: f64, note: String) -> Row {
    Row {
        name: name.into(),
        computed,
        reference,
        tolerance,
        margin: tolerance - (computed - reference).abs(),
        note,
    }
}

/// Run every mesh of the case and compare. A solver error is a failure ("cannot run").
pub fn run_case_with(case: &Case, solver: &dyn Solve) -> Report {
    if let Class::R { rate, band } = &case.class {
        return match &case.manufactured {
            Some(m) => run_rate(case, solver, *rate, *band, &m.mesh_sizes),
            None => fail(
                format!("{}: class R has no manufactured solution", case.id),
                vec![],
            ),
        };
    }
    let mut values: Vec<Vec<f64>> = Vec::new();
    for mesh in &case.meshes {
        match solver.measure(case, mesh) {
            Ok(v) if v.len() == case.references.len() => values.push(v),
            Ok(_) => {
                return fail(
                    format!("cannot run {}: wrong number of values", mesh.display()),
                    vec![],
                )
            }
            Err(e) => return fail(format!("cannot run {}: {e}", mesh.display()), vec![]),
        }
    }
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (i, r) in case.references.iter().enumerate() {
        let series: Vec<f64> = values.iter().map(|v| v[i]).collect();
        match &case.class {
            Class::N { rel_tol, abs_tol } => {
                let tol = rel_tol * r.value.abs() + abs_tol;
                let w = row(&r.name, series[0], r.value, tol, String::new());
                if check_n(series[0], r.value, *rel_tol, *abs_tol).is_err() {
                    problems.push(format!(
                        "{}: |{} - {}| exceeds {tol}",
                        r.name, series[0], r.value
                    ));
                }
                rows.push(w);
            }
            Class::L {
                ratio,
                reference_error,
                order_min,
                order_max,
            } => match richardson(&series, *ratio) {
                Err(e) => problems.push(format!("{}: {e}", r.name)),
                Ok((limit, order, estimate)) => {
                    let tol = estimate + reference_error * r.value.abs();
                    let note = format!(
                        "finest={:e} order={order:.3} estimate={estimate:e}",
                        series[series.len() - 1]
                    );
                    let w = row(&r.name, limit, r.value, tol, note);
                    if !(*order_min..=*order_max).contains(&order) {
                        problems.push(format!(
                            "{}: observed order {order:.3} is outside the band {order_min} to {order_max}",
                            r.name
                        ));
                    }
                    // An overflow makes the tolerance infinite and the margin NaN, and NaN is
                    // not below 0. Thus only a finite margin can pass.
                    if !w.margin.is_finite() {
                        problems.push(format!(
                            "{}: the tolerance {tol} or the difference |limit {limit} - {}| is not finite",
                            r.name, r.value
                        ));
                    } else if w.margin < 0.0 {
                        problems.push(format!(
                            "{}: |limit {limit} - {}| exceeds {tol}",
                            r.name, r.value
                        ));
                    }
                    rows.push(w);
                }
            },
            Class::R { .. } => unreachable!("class R returns before the loop"),
        }
    }
    if problems.is_empty() {
        Report {
            outcome: Outcome::Pass,
            rows,
        }
    } else {
        fail(problems.join("; "), rows)
    }
}

/// Class R: the observed rate is the slope of log(error) against log(h) over all meshes.
/// The error must also fall from each mesh to the next, or the rate says nothing.
fn run_rate(case: &Case, solver: &dyn Solve, rate: f64, band: f64, sizes: &[f64]) -> Report {
    let mut errors = Vec::new();
    for mesh in &case.meshes {
        match solver.field_error(case, mesh) {
            Ok(e) => errors.push(e),
            Err(e) => return fail(format!("cannot run {}: {e}", mesh.display()), vec![]),
        }
    }
    let observed = observed_rate(sizes, &errors);
    let note = format!(
        "errors={}",
        errors
            .iter()
            .map(|e| format!("{e:.4e}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let w = row(
        "rate",
        observed.as_ref().copied().unwrap_or(f64::NAN),
        rate,
        band,
        note,
    );
    let mut problems = Vec::new();
    if errors.windows(2).any(|w| w[1] >= w[0]) {
        problems.push(format!(
            "the error does not fall with the mesh size: {errors:?}"
        ));
    }
    if let Err(e) = check_r(sizes, &errors, rate, band) {
        problems.push(e);
    }
    let report = if problems.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(problems.join("; "))
    };
    Report {
        outcome: report,
        rows: vec![w],
    }
}

/// The convergence check: class L as `run_case_with`. A class N case has one mesh, so it fails.
pub fn run_convergence_with(case: &Case, solver: &dyn Solve) -> Report {
    match case.class {
        Class::N { .. } => fail(
            format!(
                "case {} is class N, which has one mesh and no convergence sequence",
                case.id
            ),
            vec![],
        ),
        _ => run_case_with(case, solver),
    }
}
