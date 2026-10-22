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
//! (see [`crate::ccx`]). Meep runs its script in the same way (see [`crate::meep`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, prepare_run_dir, supervise, CancelToken, Program};
use crate::getdp;

/// The folder of the Chrono files in a run folder.
pub const WORK: &str = "chrono";

/// A solver that a Python script of the case drives: Chrono, or Meep.
pub(crate) struct Script {
    /// The script of a case, for example `chrono.py`.
    pub file: &'static str,
    /// The Python module of the solver, for example `pychrono`.
    pub module: &'static str,
    /// The name of the module in the errors, for example `PyChrono`.
    pub name: &'static str,
    /// What to do when Python has no module `module`.
    pub install: &'static str,
    /// The folder of the solver files in a run folder.
    pub work: &'static str,
    /// The unit system of the results, and true for the true scale of a mechanism (see
    /// [`crate::vtu::write_pvd`]).
    pub units: &'static str,
    pub multibody: bool,
}

const SCRIPT: Script = Script {
    file: "chrono.py",
    module: "pychrono",
    name: "PyChrono",
    install: "Install PyChrono (README.md) and set FH_PYCHRONO to its Python",
    work: WORK,
    units: "SI-m",
    multibody: true,
};

/// True when `dir` is a Chrono case: it has `chrono.py`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(SCRIPT.file).is_file()
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
        let limits = (Instant::now(), self.timeout, self.memory_limit);
        run_script(
            &SCRIPT, &self.exe, limits, case, run_dir, cancel, on_program,
        )
    }
}

/// Solve the case `case` of the solver `script` with the Python `exe` in the folder `run_dir`:
/// start `python3 <file>` in a copy of the case, then write the views of the `.pos` files. The
/// run clears the folder first, if a run made the folder earlier. `limits` holds the start of
/// the time limit, the time limit and the memory limit, as for [`supervise`].
pub(crate) fn run_script(
    script: &Script,
    exe: &Path,
    limits: (Instant, Option<Duration>, u64),
    case: &Path,
    run_dir: &Path,
    cancel: &CancelToken,
    on_program: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let case =
        fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
    if !case.join(script.file).is_file() {
        return Err(format!("{} has no {}", case.display(), script.file));
    }
    let run_dir = prepare_run_dir(run_dir, &case)?;
    let work = run_dir.join(script.work);
    getdp::copy_case(&case, &work, false)?;
    let python = Program {
        name: "python",
        install: script.install,
    };
    on_program(python.name);
    let log_file = work.join("log.python");
    let folder = |e: std::io::Error| format!("run folder: {e}");
    let log = fs::File::create(&log_file).map_err(folder)?;
    let mut cmd = Command::new(exe);
    cmd.arg(script.file)
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(folder)?)
        .stderr(log);
    let status = supervise(&python, &mut cmd, limits, cancel, &mut || {})?;
    if !status.success() {
        let log = fs::read_to_string(&log_file).unwrap_or_default();
        if log.contains(&format!("No module named '{}'", script.module)) {
            return Err(format!(
                "{} has no {}. {}. The full output is in {}",
                exe.display(),
                script.name,
                script.install,
                log_file.display()
            ));
        }
        return Err(getdp::failure(python.name, status, &log, &log_file));
    }
    getdp::write_views(&work, &run_dir, script.units, script.multibody)
}
