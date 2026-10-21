//! The parts of `fh`: the CalculiX and OpenFOAM runners, the server with the chat, the readers of
//! meshes and STEP files, and the validation. The commands are in the binary (`main.rs`).

pub mod ccx;
pub mod daemon;
pub mod foam;
pub mod msh;
pub mod step;
pub mod validation;
pub mod vtu;
