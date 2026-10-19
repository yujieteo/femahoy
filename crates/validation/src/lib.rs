//! Harness and check file.
//!
//! The check file of a case (DESIGN.md section 10.4) and the runner that compares the values of
//! a solver with the references.

mod fixture;
mod run;

pub use fixture::{load_all, Case, Field, FixtureError, Op, Quantity, Reference};
pub use run::{check, run_case_with, Outcome, Report, Row, Solve};
