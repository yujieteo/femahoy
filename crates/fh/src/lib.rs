//! The parts of `fh`: the CalculiX, OpenFOAM, GetDP, preCICE, Chrono and LAMMPS runners, the
//! topology and shape optimization loops, the server with the chat, the readers of meshes and
//! STEP files, and the validation. The commands are in the binary (`main.rs`).

pub mod ccx;
pub mod chrono;
pub mod daemon;
pub mod foam;
pub mod getdp;
pub mod lammps;
pub mod msh;
pub mod optimize;
pub mod precice;
pub mod step;
pub mod topology;
pub mod validation;
pub mod vtu;
