//! Harness, tolerance classes, manufactured solutions.
//!
//! The fixture schema, the checks for tolerance classes L, N and R (DESIGN.md section 10.3),
//! and the runner that compares the values of a solver with the references.

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
pub use model::{
    BoundarySpec, CloadSpec, DsloadSpec, MaterialSpec, ModelSpec, RegionSpec, Select, SetSpec,
    RESERVED_NAMES,
};
pub use run::{run_case_with, run_convergence_with, Outcome, Report, Row, Solve};
pub use tolerance::{check_l, check_n, check_r, observed_rate, richardson};
