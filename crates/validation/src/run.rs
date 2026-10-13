//! Run a case with a solver and compare the numbers with the references.

use std::path::Path;

use crate::{check_n, richardson, Case, Class};

/// A solver that measures the quantities of a case on one mesh.
pub trait Solve {
    /// The value of each reference of `case`, in the order of `case.references`.
    fn measure(&self, case: &Case, mesh: &Path) -> Result<Vec<f64>, String>;
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
                    if w.margin < 0.0 {
                        problems.push(format!(
                            "{}: |limit {limit} - {}| exceeds {tol}",
                            r.name, r.value
                        ));
                    }
                    rows.push(w);
                }
            },
            Class::R { .. } => {
                return fail(
                    format!(
                        "{}: class R cannot run, the fixture has no mesh size",
                        case.id
                    ),
                    rows,
                )
            }
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
