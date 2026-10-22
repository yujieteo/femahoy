//! openEMS as the solver of femahoy for antennas and other high-frequency electromagnetics, in
//! the process of `fh`.
//!
//! An openEMS case is a folder with `openems.py`, a script for the Python interface of openEMS
//! (the modules `CSXCAD` and `openEMS`). [`OpenEms::run`] copies the files of the folder to
//! `<run-dir>/openems` and starts there the Python of openEMS (`$FH_OPENEMS`, else `python3` on
//! PATH): `python3 openems.py`. The script builds the model, runs the FDTD solver of openEMS and
//! writes its results. The output of Python goes to `<run-dir>/openems/log.python`.
//!
//! The script writes its text results (tables) to the files that it names, and fields for the
//! page to `.pos` files. Then the run writes the views of the `.pos` files to `step-N.vtu` and
//! `results.pvd`, as a GetDP run does (see [`crate::getdp`]). The units are SI-m.
//!
//! openEMS is GPL, so femahoy only starts it as a separate program. A run has the limits of a
//! run of `ccx` (see [`crate::ccx`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, CancelToken};
use crate::chrono::{run_script, Script};

/// The folder of the openEMS files in a run folder.
pub const WORK: &str = "openems";

const SCRIPT: Script = Script {
    file: "openems.py",
    module: "CSXCAD",
    name: "openEMS",
    install: "Install openEMS with its Python interface (README.md) and set FH_OPENEMS to its \
              Python",
    work: WORK,
    units: "SI-m",
    multibody: false,
};

/// True when `dir` is an openEMS case: it has `openems.py`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(SCRIPT.file).is_file()
}

/// The Python of openEMS, with the limits of its runs.
#[derive(Debug, Clone)]
pub struct OpenEms {
    /// `$FH_OPENEMS`, else `python3` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of Python in bytes, as for `ccx`. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for OpenEms {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenEms {
    /// Python from `$FH_OPENEMS`, else `python3` on PATH. It has no time limit. The memory limit
    /// is 85 % of the machine RAM.
    pub fn new() -> Self {
        OpenEms {
            exe: std::env::var_os("FH_OPENEMS")
                .map_or_else(|| PathBuf::from("python3"), PathBuf::from),
            timeout: None,
            memory_limit: default_memory_limit(),
        }
    }

    /// Solve the case `case` in the folder `run_dir`. The run clears the folder first, if a run
    /// made the folder earlier.
    pub fn run(&self, case: &Path, run_dir: &Path) -> Result<(), String> {
        self.run_with(case, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Solve, and stop when `cancel` is set. `on_program` gets the name of the program that the
    /// run starts.
    pub fn run_with(
        &self,
        case: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_program: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        let limits = (Instant::now(), self.timeout, self.memory_limit);
        run_script(
            &SCRIPT, &self.exe, limits, case, run_dir, cancel, on_program,
        )
    }
}
