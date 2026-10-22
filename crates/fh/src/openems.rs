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

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, prepare_run_dir, supervise, CancelToken, Program};
use crate::getdp;

/// The folder of the openEMS files in a run folder.
pub const WORK: &str = "openems";

/// What to do when Python has no openEMS.
const INSTALL: &str = "Install openEMS with its Python interface (README.md) and set FH_OPENEMS \
                       to its Python";

/// True when `dir` is an openEMS case: it has `openems.py`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("openems.py").is_file()
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
        let start = Instant::now();
        let case =
            fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
        if !is_case(&case) {
            return Err(format!(
                "{} is not an openEMS case: it has no openems.py",
                case.display()
            ));
        }
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        getdp::copy_case(&case, &work, false)?;
        let python = Program {
            name: "python",
            install: INSTALL,
        };
        on_program(python.name);
        let log_file = work.join("log.python");
        let folder = |e: std::io::Error| format!("run folder: {e}");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("openems.py")
            .current_dir(&work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let limits = (start, self.timeout, self.memory_limit);
        let status = supervise(&python, &mut cmd, limits, cancel, &mut || {})?;
        if !status.success() {
            let log = fs::read_to_string(&log_file).unwrap_or_default();
            if log.contains("No module named 'openEMS'") || log.contains("No module named 'CSXCAD'")
            {
                return Err(format!(
                    "{} has no openEMS. {INSTALL}. The full output is in {}",
                    self.exe.display(),
                    log_file.display()
                ));
            }
            return Err(getdp::failure(python.name, status, &log, &log_file));
        }
        getdp::write_views(&work, &run_dir, "SI-m", false)
    }
}
