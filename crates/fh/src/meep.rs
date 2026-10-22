//! Meep as the solver of femahoy for optics and photonics, in the process of `fh`.
//!
//! A Meep case is a folder with `meep_case.py`, a script for Meep, the FDTD solver with a Python
//! interface. The name is not `meep.py`, because Python then imports the script as `meep`.
//! [`Meep::run`] copies the files of the folder to `<run-dir>/meep` and starts there the user's
//! Python with Meep (`$FH_MEEP`, else `python3` on PATH): `python3 meep_case.py`. The output of
//! Python goes to `<run-dir>/meep/log.python`.
//!
//! The script writes its text results (tables) to the files that it names, and its fields to
//! `.pos` files. Then the run writes the views of the `.pos` files to `step-N.vtu` and
//! `results.pvd`, as a Chrono run does (see [`crate::chrono`]). Meep has no length unit, so the
//! unit system is `unspecified`.
//!
//! Meep is GPL, so femahoy only starts it as a separate program. A run has the limits of a run
//! of `ccx` (see [`crate::ccx`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, CancelToken};
use crate::chrono::{run_script, Script};

/// The folder of the Meep files in a run folder.
pub const WORK: &str = "meep";

const SCRIPT: Script = Script {
    file: "meep_case.py",
    module: "meep",
    name: "Meep",
    install: "Install Meep (README.md) and set FH_MEEP to its Python",
    work: WORK,
    units: "unspecified",
    multibody: false,
};

/// True when `dir` is a Meep case: it has `meep_case.py`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(SCRIPT.file).is_file()
}

/// The Python with Meep, with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Meep {
    /// `$FH_MEEP`, else `python3` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of Python in bytes, as for `ccx`. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Meep {
    fn default() -> Self {
        Self::new()
    }
}

impl Meep {
    /// Python from `$FH_MEEP`, else `python3` on PATH. It has no time limit. The memory limit is
    /// 85 % of the machine RAM.
    pub fn new() -> Self {
        Meep {
            exe: std::env::var_os("FH_MEEP")
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
