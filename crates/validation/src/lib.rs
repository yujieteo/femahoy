//! Harness and fixture schema.
//!
//! The fixture schema (DESIGN.md section 10.4) and the runner that compares the values of a
//! solver with the references.

mod fixture;
mod model;
mod run;

pub use fixture::{
    load_all, Case, Field, FixtureError, Op, Quantity, Reference, Units, SCHEMA_VERSION,
};
pub use model::{
    BoundarySpec, CloadSpec, DsloadSpec, MaterialSpec, ModelSpec, RegionSpec, Select, SetSpec,
    RESERVED_NAMES,
};
pub use run::{check, run_case_with, Outcome, Report, Row, Solve};
