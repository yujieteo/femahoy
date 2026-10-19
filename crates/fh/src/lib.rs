//! The parts of `fh`: the CalculiX runner, the server with the chat, the readers and
//! checks of meshes, and the validation. The commands are in the binary (`main.rs`).

pub mod ccx;
pub mod check;
pub mod daemon;
pub mod msh;
pub mod step;
pub mod validation;
pub mod vtu;
