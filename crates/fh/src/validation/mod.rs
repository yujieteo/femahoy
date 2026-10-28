//! Validation: the check file of a case (DESIGN.md section 7), the solve with `ccx`, and the
//! comparison of the numbers with the references.

mod fixture;
mod measure;
mod run;

pub use fixture::{load_all, Case, Field};
pub use measure::{Native, Subprocess};
pub use run::{check, run_case_with, summary, Outcome, Report, Solve};
