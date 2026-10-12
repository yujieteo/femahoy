//! gmsh `.msh` import, VTU and PVD write and read.

mod msh;

pub use msh::{read_msh, MshError};
