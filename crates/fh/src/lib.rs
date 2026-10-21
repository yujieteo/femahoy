//! The parts of `fh`: the CalculiX and OpenFOAM runners, the server with the chat, the readers and
//! checks of meshes, and the validation. The commands are in the binary (`main.rs`).

pub mod ccx;
pub mod check;
pub mod daemon;
pub mod foam;
pub mod msh;
pub mod step;
pub mod validation;
pub mod vtu;
