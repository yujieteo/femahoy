//! agent-facing summaries and queries.
//!
//! This crate implements the TOON output schemas in `toon-schema.md`. Only the
//! `wrap` module calls `toon-format` (rule R1).
//!
//! A later `toon-format` release may change the output. The exact version pin
//! and the snapshot tests guard against this. TOON is for agent reading only:
//! VTU files hold the full-precision data (rule R3).

mod check;
mod common;
mod run;
mod validate;
mod wrap;

pub use check::{Check, ElementType, MaterialRow, QualityRow, SetRow, StepRow};
pub use common::{Diag, ExitCode, Header, Next, Severity, Status, MAX_NEXT, VERSION};
pub use run::{ContactRow, Failure, IncResult, Increment, Residual, Run, RunStep, Worst};
pub use validate::{
    CaseRow, CaseStatus, Quantity, Rate, Refinement, Tolerance, Validate, ValidateCase,
};
pub use wrap::{Cell, Delim, Doc, Error, Opts, Table, MAX_FIELDS};
