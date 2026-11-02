//! Skein as a solver of femahoy, in the process of `fh`.
//!
//! A Skein case is a folder with `model.skn`, a model in the Skein language: typed, with units
//! checked. [`Skein::run`] copies the files of the folder to `<run-dir>/skein` and starts there
//! `skein report model.skn -o report.html` (Skein is `$FH_SKEIN`, else `skein` on PATH). Skein
//! checks the model, solves it, and writes the report, an HTML file with the source, the tables
//! and a viewer. The output of Skein and its tables (the checks, the solve, the `expect` values) go to
//! `<run-dir>/skein/log.skein`.
//!
//! The report is not a VTU file, so the page shows it through the report ticket of the daemon
//! ([`publish_report`] moves it to `<run>/report.html`). The report never gets the token of the
//! daemon. A run has the limits of a run of `ccx` (see [`crate::ccx`]), but the memory limit counts
//! resident memory only, because a Goose program reserves a large address space. The memory limit is also
//! the memory budget of Skein (`SKEIN_MEMORY_BUDGET`), so Skein refuses a model that does not
//! fit, before it starts the solve.
//!
//! When Skein reports errors (type, unit or other), the run fails and its message holds them,
//! so that the agent can correct the model and run it again. A model with a failed `expect`
//! also fails the run, and keeps its report.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::ccx::{default_memory_limit, prepare_run_dir, supervise_with, CancelToken, Program};
use crate::getdp;

/// The folder of the Skein files in a run folder.
pub const WORK: &str = "skein";
/// The model of a case.
pub const MODEL: &str = "model.skn";
/// The report that Skein writes in the folder of the files.
pub const REPORT: &str = "report.html";
/// The output of Skein.
pub const LOG: &str = "log.skein";

const INSTALL: &str =
    "Install Skein (github.com/yujieteo/skein, ./build.sh) and put skein on PATH, \
                       or set FH_SKEIN to its path";

/// True when `dir` is a Skein case: it has `model.skn`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(MODEL).is_file()
}

/// The `skein` program, with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Skein {
    /// `$FH_SKEIN`, else `skein` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit.
    pub timeout: Option<Duration>,
    /// Limit for the process group of Skein in bytes, as for `ccx`. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Skein {
    fn default() -> Self {
        Self::new()
    }
}

impl Skein {
    /// `$FH_SKEIN`, else `skein` on PATH. It has no time limit. The memory limit is 85 % of the
    /// machine RAM.
    pub fn new() -> Self {
        Skein {
            exe: std::env::var_os("FH_SKEIN").map_or_else(|| PathBuf::from("skein"), PathBuf::from),
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
        let case =
            fs::canonicalize(case).map_err(|e| format!("{}: case folder: {e}", case.display()))?;
        if !is_case(&case) {
            return Err(format!("{} has no {MODEL}", case.display()));
        }
        let run_dir = prepare_run_dir(run_dir, &case)?;
        let work = run_dir.join(WORK);
        getdp::copy_case(&case, &work, false)?;
        let program = Program {
            name: "skein",
            install: INSTALL,
        };
        on_program(program.name);
        let log_file = work.join(LOG);
        let folder = |e: std::io::Error| format!("run folder: {e}");
        let log = fs::File::create(&log_file).map_err(folder)?;
        let mut cmd = Command::new(&self.exe);
        cmd.args(["report", MODEL, "-o", REPORT])
            .current_dir(&work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(folder)?)
            .stderr(log);
        if self.memory_limit != u64::MAX {
            cmd.env("SKEIN_MEMORY_BUDGET", self.memory_limit.to_string());
        }
        let limits = (Instant::now(), self.timeout, self.memory_limit);
        // Skein is a Goose program. Goose reserves a large address space at the start, so the
        // limit is on resident memory only (the probe), and Skein keeps its own budget.
        let status = supervise_with(&program, &mut cmd, limits, cancel, &mut || {}, false)?;
        append_tables(&work, &log_file);
        if status.success() {
            return Ok(());
        }
        let log = fs::read_to_string(&log_file).unwrap_or_default();
        Err(failure(&status.to_string(), &log, &log_file))
    }
}

/// Add the tables of `skein report` (`check`, `verify` and `run`, in `.skein/report/<model>/`) to
/// the log, because `skein report` prints only the path of the report.
fn append_tables(work: &Path, log_file: &Path) {
    use std::io::Write;
    let Ok(mut log) = fs::OpenOptions::new().append(true).open(log_file) else {
        return;
    };
    let models = fs::read_dir(work.join(".skein/report"))
        .into_iter()
        .flatten()
        .flatten();
    for model in models {
        for name in ["check.toon", "verify.toon", "run.toon"] {
            if let Ok(text) = fs::read_to_string(model.path().join(name)) {
                let _ = writeln!(log, "{text}");
            }
        }
    }
}

/// Why Skein failed: the lines of its output that hold an error or a failed check, else the last
/// lines.
fn failure(status: &str, log: &str, log_file: &Path) -> String {
    let lines: Vec<&str> = log
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("error,") || l.contains(",fail,") || l.starts_with("Error"))
        .collect();
    let see = format!("The full output is in {}", log_file.display());
    if lines.is_empty() {
        let tail: Vec<&str> = log.lines().rev().take(8).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        return format!(
            "skein failed ({status}). {see}. Its last lines: {}",
            tail.join(" | ")
        );
    }
    format!(
        "skein failed ({status}): {}. Correct the model as the message tells. {see}.",
        lines.join(" | ")
    )
}

/// Make the report of a run the report of the page: move `<run-dir>/skein/report.html` to
/// `report.html` beside the run folder `run_dir` (the folder `out` of a run in the state folder).
/// It does nothing when Skein wrote no report.
pub fn publish_report(run_dir: &Path) -> Result<(), String> {
    let report = run_dir.join(WORK).join(REPORT);
    let Some(run) = run_dir.parent().filter(|_| report.is_file()) else {
        return Ok(());
    };
    let temporary = run.join(".report-skein");
    fs::copy(&report, &temporary).map_err(|e| format!("report: {e}"))?;
    fs::rename(&temporary, run.join("report.html")).map_err(|e| format!("report: {e}"))
}
