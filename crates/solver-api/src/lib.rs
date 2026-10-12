//! The `Solver` trait, the subprocess protocol (version 1) and the registry.
//!
//! # Protocol v1
//!
//! The runner starts `exe <input.inp> <run-dir>` with `run-dir` as the working
//! folder. The solver must:
//!
//! 1. print `fh-solver-protocol 1` as the first line on stdout,
//! 2. write `results.pvd`, one VTU file for each step that the PVD lists,
//!    and `summary.toon` in `run-dir`,
//! 3. exit with code 0 on success. On failure it exits with a non-zero code
//!    and prints the reason on stderr.
//!
//! Results from a solver that did not pass the validation suite are marked
//! unvalidated.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fh_inp::Model;
use fh_toon::Document;

/// First line that a protocol v1 solver prints on stdout.
pub const PROTOCOL_HEADER: &str = "fh-solver-protocol 1";
pub const PVD_FILE: &str = "results.pvd";
pub const SUMMARY_FILE: &str = "summary.toon";
/// Default time limit for a subprocess run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError(pub String);

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApiError {}

fn fail<T>(message: impl Into<String>) -> Result<T, ApiError> {
    Err(ApiError(message.into()))
}

/// What a solver run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct RunReport {
    pub pvd: PathBuf,
    pub summary: Document,
    /// False until the solver passes the validation suite.
    pub validated: bool,
}

/// A trusted solver that runs inside the process.
pub trait Solver {
    fn name(&self) -> &str;
    /// True when the solver passed the validation suite.
    fn validated(&self) -> bool;
    /// Solve `model` and write the protocol files in `run_dir`.
    fn solve(&self, model: &Model, run_dir: &Path) -> Result<RunReport, ApiError>;
}

/// Solvers by name.
#[derive(Default)]
pub struct Registry {
    solvers: BTreeMap<String, Box<dyn Solver>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a solver. A second solver with the same name is an error.
    pub fn register(&mut self, solver: Box<dyn Solver>) -> Result<(), ApiError> {
        let name = solver.name().to_owned();
        if self.solvers.contains_key(&name) {
            return fail(format!("solver {name} is already registered"));
        }
        self.solvers.insert(name, solver);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Solver> {
        self.solvers.get(name).map(|b| &**b)
    }

    pub fn names(&self) -> Vec<&str> {
        self.solvers.keys().map(String::as_str).collect()
    }
}

/// A solver in another executable that speaks protocol v1.
#[derive(Debug, Clone)]
pub struct SubprocessSolver {
    pub exe: PathBuf,
    pub timeout: Duration,
    pub validated: bool,
}

impl SubprocessSolver {
    /// A new subprocess solver. It is unvalidated until the suite passes.
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        SubprocessSolver {
            exe: exe.into(),
            timeout: DEFAULT_TIMEOUT,
            validated: false,
        }
    }

    /// Run the solver on `inp` in the new folder `run_dir`.
    pub fn run(&self, inp: &Path, run_dir: &Path) -> Result<RunReport, ApiError> {
        fs::create_dir_all(run_dir).map_err(|e| ApiError(format!("run folder: {e}")))?;
        let inp = fs::canonicalize(inp).map_err(|e| ApiError(format!("input file: {e}")))?;
        let run_dir =
            fs::canonicalize(run_dir).map_err(|e| ApiError(format!("run folder: {e}")))?;
        let open = |name: &str| {
            fs::File::create(run_dir.join(name)).map_err(|e| ApiError(format!("{name}: {e}")))
        };
        let mut child = Command::new(&self.exe)
            .arg(&inp)
            .arg(&run_dir)
            .current_dir(&run_dir)
            .stdin(Stdio::null())
            .stdout(open("stdout.txt")?)
            .stderr(open("stderr.txt")?)
            .spawn()
            .map_err(|e| ApiError(format!("cannot start {}: {e}", self.exe.display())))?;
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if start.elapsed() > self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return fail(format!(
                        "solver stopped after the time limit of {} s",
                        self.timeout.as_secs_f64()
                    ));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => return fail(format!("wait failed: {e}")),
            }
        };
        let read = |name: &str| fs::read_to_string(run_dir.join(name)).unwrap_or_default();
        if !status.success() {
            return fail(format!(
                "solver failed ({status}): {}",
                read("stderr.txt").trim()
            ));
        }
        if read("stdout.txt").lines().next() != Some(PROTOCOL_HEADER) {
            return fail(format!(
                "solver does not speak protocol v1: first stdout line is not \"{PROTOCOL_HEADER}\""
            ));
        }
        let summary = fs::read_to_string(run_dir.join(SUMMARY_FILE))
            .map_err(|e| ApiError(format!("{SUMMARY_FILE}: {e}")))?;
        let summary =
            Document::parse(&summary).map_err(|e| ApiError(format!("{SUMMARY_FILE}: {e}")))?;
        let pvd = run_dir.join(PVD_FILE);
        let pvd_text =
            fs::read_to_string(&pvd).map_err(|e| ApiError(format!("{PVD_FILE}: {e}")))?;
        let listed: Vec<&str> = pvd_text
            .split("file=\"")
            .skip(1)
            .filter_map(|r| r.split('"').next())
            .collect();
        if listed.is_empty() {
            return fail(format!("{PVD_FILE} lists no VTU file"));
        }
        for f in listed {
            if !run_dir.join(f).is_file() {
                return fail(format!("{PVD_FILE} lists {f}, but the file is missing"));
            }
        }
        Ok(RunReport {
            pvd,
            summary,
            validated: self.validated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;
    impl Solver for Dummy {
        fn name(&self) -> &str {
            "dummy"
        }
        fn validated(&self) -> bool {
            false
        }
        fn solve(&self, _: &Model, _: &Path) -> Result<RunReport, ApiError> {
            fail("not a real solver")
        }
    }

    #[test]
    fn registry_finds_by_name_and_rejects_duplicates() {
        let mut r = Registry::new();
        r.register(Box::new(Dummy)).unwrap();
        assert!(r.register(Box::new(Dummy)).is_err());
        assert_eq!(r.names(), ["dummy"]);
        assert!(r.get("dummy").is_some());
        assert!(r.get("other").is_none());
    }
}
