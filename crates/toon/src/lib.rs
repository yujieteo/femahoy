//! agent-facing summaries and queries.
//!
//! This crate implements the TOON output schemas in `toon-schema.md`. Only the
//! `wrap` module calls `toon-format` (rule R1).
//!
//! A later `toon-format` release may change the output. The exact version pin
//! and the snapshot tests guard against this. TOON is for agent reading only:
//! VTU files hold the exact values (rule R3).
//!
//! TOON numbers are rounded to `precision` significant figures (default 4).
//! Every output states this in its `precision` header key. The exact values
//! are in the VTU files.
//! Decision D-03, see DESIGN.md.
//!
//! The schema is experimental until a command uses it.

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
