//! Harness, tolerance classes, manufactured solutions.
//!
//! M1: fixture schema, the checks for tolerance classes L, N and R
//! (DESIGN.md section 10.3), and a runner that reports `no solver`.

mod fixture;
mod tolerance;

pub use fixture::{load_all, Case, Class, FixtureError, Reference};
pub use tolerance::{check_l, check_n, check_r, richardson};

/// The result of one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No solver exists yet, so the case cannot run.
    NoSolver,
    Pass,
    Fail(String),
}

/// Run one case. In M1 no solver exists, so every case reports `NoSolver`.
pub fn run_case(_case: &Case) -> Outcome {
    Outcome::NoSolver
}
