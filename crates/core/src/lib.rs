//! The readers that `fh` and the 3D view share: CalculiX `.inp` text (`inp`), VTU and PVD
//! results (`vtu`) and the `grid.toml` case of the built-in grid solver (`grid`). The crate
//! compiles to wasm for the 3D view.

pub mod grid;
pub mod inp;
pub mod vtu;
