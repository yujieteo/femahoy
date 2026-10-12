//! The subprocess protocol (version 1) for solvers in other executables.
//!
//! # Protocol v1
//!
//! The runner starts `exe <input.inp> <run-dir>` with `run-dir` as the working
//! folder. The solver must:
//!
//! 1. print `fh-solver-protocol 1` as the first line on stdout,
//! 2. write `results.pvd`, one VTU file for each step that the PVD lists,
//!    and `summary.txt` in `run-dir`,
//! 3. exit with code 0 on success. On failure it exits with a non-zero code
//!    and prints the reason on stderr.
//!
//! `summary.txt` is **provisional**. The TOON summary schema is an open design
//! item, so the runner returns the text as it is and does not parse it.
//!
//! The runner stops a solver that runs longer than the time limit or uses more
//! resident memory than the memory limit. The memory check covers the solver
//! process, not the processes that it starts. Results are always unvalidated.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// First line that a protocol v1 solver prints on stdout.
pub const PROTOCOL_HEADER: &str = "fh-solver-protocol 1";
pub const PVD_FILE: &str = "results.pvd";
/// Provisional plain text summary. Not a TOON schema.
pub const SUMMARY_FILE: &str = "summary.txt";

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub pvd: PathBuf,
    /// The text of `summary.txt`. Provisional.
    pub summary: String,
}

/// A solver in another executable that speaks protocol v1.
#[derive(Debug, Clone)]
pub struct SubprocessSolver {
    pub exe: PathBuf,
    pub timeout: Duration,
    /// Limit for the resident memory of the solver process, in bytes.
    pub memory_limit: u64,
}

impl SubprocessSolver {
    /// A solver with a 10 minute time limit and a 4 GiB memory limit.
    /// Both values are provisional.
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        SubprocessSolver {
            exe: exe.into(),
            timeout: Duration::from_secs(600),
            memory_limit: 4 << 30,
        }
    }

    /// Run the solver on `inp` in the folder `run_dir`.
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
                Ok(None) => {}
                Err(e) => return fail(format!("wait failed: {e}")),
            }
            let reason = if start.elapsed() > self.timeout {
                Some(format!(
                    "solver stopped after the time limit of {} s",
                    self.timeout.as_secs_f64()
                ))
            } else if resident_bytes(child.id()).is_some_and(|b| b > self.memory_limit) {
                Some(format!(
                    "solver stopped after it used more than the memory limit of {} bytes",
                    self.memory_limit
                ))
            } else {
                None
            };
            if let Some(reason) = reason {
                let _ = child.kill();
                let _ = child.wait();
                return fail(reason);
            }
            std::thread::sleep(Duration::from_millis(10));
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
        Ok(RunReport { pvd, summary })
    }
}

/// Resident memory of a process in bytes, from `ps`. `None` if `ps` fails.
fn resident_bytes(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let kib: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib * 1024)
}
