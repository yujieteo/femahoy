//! Project Chrono as the multibody solver of femahoy, in the process of `fh`.
//!
//! A Chrono case is a folder with `chrono.py`, a script for PyChrono, the Python module of
//! Chrono. [`Chrono::run`] copies the files of the folder to `<run-dir>/chrono` and starts there
//! the user's Python with PyChrono (`$FH_PYCHRONO`, else `python3` on PATH): `python3 chrono.py`.
//! The output of Python goes to `<run-dir>/chrono/log.python`.
//!
//! The script writes its text results (tables) to the files that it names, and the motion of the
//! bodies to `.pos` files. Then the run writes the views of the `.pos` files to `step-N.vtu` and
//! `results.pvd`, as a GetDP run does (see [`crate::getdp`]): the view `U` is the displacement of
//! the page. The units are SI-m, and `results.pvd` has the kind `multibody`, so the page shows
//! the bodies at true scale.
//!
//! Chrono stays a separate program, as each other solver. A run has the limits of a run of `ccx`
//! (see [`crate::ccx`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, prepare_run_dir, supervise, CancelToken, Program};
use crate::getdp;

/// The folder of the Chrono files in a run folder.
pub const WORK: &str = "chrono";

/// What to do when Python has no PyChrono.
const INSTALL: &str = "Install PyChrono (README.md) and set FH_PYCHRONO to its Python";

/// True when `dir` is a Chrono case: it has `chrono.py`.
pub fn is_case(dir: &Path) -> bool {
    dir.join("chrono.py").is_file()
}

/// The Python with PyChrono, with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Chrono {
    /// `$FH_PYCHRONO`, else `python3` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of Python in bytes, as for `ccx`. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Chrono {
    fn default() -> Self {
        Self::new()
    }
}

impl Chrono {
    /// Python from `$FH_PYCHRONO`, else `python3` on PATH. It has no time limit. The memory
    /// limit is 85 % of the machine RAM.
    pub fn new() -> Self {
        Chrono {
            exe: std::env::var_os("FH_PYCHRONO")
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
                "{} is not a Chrono case: it has no chrono.py",
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
        cmd.arg("chrono.py")
            .current_dir(&work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        let limits = (start, self.timeout, self.memory_limit);
        let status = supervise(&python, &mut cmd, limits, cancel, &mut || {})?;
        if !status.success() {
            let log = fs::read_to_string(&log_file).unwrap_or_default();
            if log.contains("No module named 'pychrono'") {
                return Err(format!(
                    "{} has no PyChrono. {INSTALL}. The full output is in {}",
                    self.exe.display(),
                    log_file.display()
                ));
            }
            return Err(getdp::failure(python.name, status, &log, &log_file));
        }
        getdp::write_views(&work, &run_dir, "SI-m", true)
    }
}
