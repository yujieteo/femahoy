//! Elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM,
//! birth and death, submodelling.
//!
//! M2: linear static analysis with `C3D8`, `C3D8I` and `C3D10`.

mod assembly;
mod constraint;
pub mod contribution;
mod dense;
pub mod element;
mod mesh;
mod output;
pub mod restart;
mod sparse;
pub mod state;
mod step;
mod system;
mod text;

pub use output::{
    input_hash, run_model, write_results, CoreSolver, Input, ORPHAN_CODE, RESTART_FILE,
};
pub use system::{
    estimate_bytes, solve, solve_from, solve_options, solve_with, step_times, Formats, Increment,
    MaterialFactory, Method, Observer, Options, Resume, Sizes, Solution, StepInfo, StepResult,
    Summary, CANCELLED, DIRECT_MAX_DOFS, RESIDUAL_LIMIT,
};
pub use text::TEXT_HEADER;

use std::fmt;

/// An error from the solver. The text is for people to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolverError(String);

impl SolverError {
    pub fn new(message: impl Into<String>) -> Self {
        SolverError(message.into())
    }
}

impl fmt::Display for SolverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SolverError {}
