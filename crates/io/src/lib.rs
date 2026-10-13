//! gmsh `.msh` import, VTU, PVD and legacy VTK write and read.

mod msh;
pub mod vtk;
pub mod vtu;

pub use msh::{read_msh, MshError};
