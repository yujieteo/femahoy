//! The parts of `fh`: the CalculiX, OpenFOAM, GetDP, preCICE, Chrono and LAMMPS runners, the
//! topology optimization loop, the server with the chat, the readers of meshes and STEP files,
//! and the validation. The commands are in the binary (`main.rs`).

pub mod ccx;
pub mod chrono;
pub mod daemon;
pub mod foam;
pub mod getdp;
pub mod lammps;
pub mod msh;
pub mod precice;
pub mod step;
pub mod topology;
pub mod validation;
pub mod vtu;

use std::path::Path;

/// The solver of a run: CalculiX for an input file (`.inp`), else the solver of the case folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solver {
    Ccx,
    OpenFoam,
    GetDp,
    Precice,
    Topology,
    Chrono,
    Lammps,
}

/// The solver of the case folder `dir`, from the file that each kind of case has. `None` when
/// `dir` is not a case folder. When `dir` has the files of 2 kinds, the first in this order wins.
pub fn solver(dir: &Path) -> Option<Solver> {
    if foam::is_case(dir) {
        Some(Solver::OpenFoam)
    } else if getdp::is_case(dir) {
        Some(Solver::GetDp)
    } else if precice::is_case(dir) {
        Some(Solver::Precice)
    } else if lammps::is_case(dir) {
        Some(Solver::Lammps)
    } else if topology::is_case(dir) {
        Some(Solver::Topology)
    } else if chrono::is_case(dir) {
        Some(Solver::Chrono)
    } else {
        None
    }
}
