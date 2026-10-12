//! gmsh `.msh` import, VTU and PVD write and read.

mod msh;
pub mod vtu;

pub use msh::{read_msh, MshError};
