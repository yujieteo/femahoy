//! The readers that `fh` and the 3D view share: CalculiX `.inp` text (`inp`) and VTU and PVD
//! results (`vtu`). The crate compiles to wasm for the 3D view.

pub mod inp;
pub mod vtu;
