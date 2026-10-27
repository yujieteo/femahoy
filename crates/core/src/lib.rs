//! The readers that `fh` and the 3D view share: CalculiX `.inp` text (`inp`), VTU and PVD
//! results (`vtu`) and the `grid.toml` case of the built-in grid solver (`grid`, and `line` for a
//! transmission line). The crate
//! compiles to wasm for the 3D view.

pub mod grid;
pub mod inp;
pub mod line;
pub mod vtu;
