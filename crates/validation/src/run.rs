//! Run a case with a solver and compare the numbers with the references.

use std::path::Path;

use crate::Case;

/// A solver that measures the quantities of a case on its input.
pub trait Solve {
    /// The value of each reference of `case`, in the order of `case.references`.
    fn measure(&self, case: &Case, input: &Path) -> Result<Vec<f64>, String>;
}

/// The result of one case.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Pass,
    Fail(String),
}

/// One compared number.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub computed: f64,
    pub reference: f64,
    /// The largest difference that passes.
    pub tolerance: f64,
    /// `tolerance - |computed - reference|`. Negative means a failure.
    pub margin: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    pub rows: Vec<Row>,
}

/// The pass rule: `|computed - reference| <= rel_tol * |reference| + abs_tol`.
pub fn check(computed: f64, reference: f64, rel_tol: f64, abs_tol: f64) -> Result<(), String> {
    if [computed, reference, rel_tol, abs_tol]
        .iter()
        .any(|v| !v.is_finite())
        || rel_tol < 0.0
        || abs_tol < 0.0
    {
        return Err(
            "result and reference must be finite, and tolerances non-negative and finite".into(),
        );
    }
    let allowed = rel_tol * reference.abs() + abs_tol;
    let diff = (computed - reference).abs();
    if !allowed.is_finite() || !diff.is_finite() {
        return Err("allowed error and difference must be finite".into());
    }
    if diff <= allowed {
        Ok(())
    } else {
        Err(format!(
            "|{computed} - {reference}| = {diff} exceeds {allowed}"
        ))
    }
}

/// Run the input of the case and compare. A solver error is a failure ("cannot run").
pub fn run_case_with(case: &Case, solver: &dyn Solve) -> Report {
    let fail = |why: String, rows| Report {
        outcome: Outcome::Fail(why),
        rows,
    };
    let values = match solver.measure(case, &case.input) {
        Ok(v) if v.len() == case.references.len() => v,
        Ok(_) => {
            return fail(
                format!(
                    "cannot run {}: wrong number of values",
                    case.input.display()
                ),
                vec![],
            )
        }
        Err(e) => return fail(format!("cannot run {}: {e}", case.input.display()), vec![]),
    };
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    for (r, computed) in case.references.iter().zip(values) {
        let tolerance = case.rel_tol * r.value.abs() + case.abs_tol;
        if let Err(e) = check(computed, r.value, case.rel_tol, case.abs_tol) {
            problems.push(format!("{}: {e}", r.name));
        }
        rows.push(Row {
            name: r.name.clone(),
            computed,
            reference: r.value,
            tolerance,
            margin: tolerance - (computed - r.value).abs(),
        });
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
