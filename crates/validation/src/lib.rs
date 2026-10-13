//! Harness, tolerance classes, manufactured solutions.
//!
//! M1: fixture schema, the checks for tolerance classes L, N and R
//! (DESIGN.md section 10.3), and a runner that reports `no solver`.

mod expr;
mod fixture;
mod model;
mod run;
mod tolerance;

pub use expr::Expr;
pub use fixture::{
    load_all, Case, Class, Field, FixtureError, Manufactured, Op, Quantity, Reference, Units,
    ELEMENT_TYPES, SCHEMA_VERSION,
};
pub use model::{BoundarySpec, CloadSpec, MaterialSpec, ModelSpec, SetSpec};
pub use run::{run_case_with, run_convergence_with, Outcome, Report, Row, Solve};
pub use tolerance::{check_l, check_n, check_r, observed_rate, richardson};

/// Run one case. In M1 no solver exists, so every case reports `NoSolver`.
pub fn run_case(_case: &Case) -> Outcome {
    Outcome::NoSolver
}

/// Run the convergence check of one case. It needs a mesh sequence, so class N has none.
/// In M1 no solver exists, so a class L or R case reports `NoSolver`.
pub fn run_convergence(case: &Case) -> Outcome {
    match case.class {
        Class::N { .. } => Outcome::Fail(format!(
            "case {} is class N, which has one mesh and no convergence sequence",
            case.id
        )),
        _ => Outcome::NoSolver,
    }
}
