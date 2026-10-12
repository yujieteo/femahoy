//! Elements, DOF elimination, Newton with cutback, contact, plasticity, NLGEOM,
//! birth and death, submodelling.
//!
//! M2: linear static analysis with `C3D8`, `C3D8I` and `C3D10`.

mod dense;
pub mod element;

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
