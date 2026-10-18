//! The `Solver` trait, the registry and the subprocess protocol (version 2).
//!
//! # Protocol v2
//!
//! The runner starts `exe <input.inp> <run-dir>` with `run-dir` as the working
//! folder. For a restart it adds `--restart` as the third argument. The runner
//! leaves the files of the earlier run in place then. The solver must:
//!
//! 1. print `fh-solver-protocol 2` as the first line on stdout,
//! 2. print events on stdout while it works (see below),
//! 3. write `results.pvd`, one VTU file for each step that the PVD lists, and
//!    `summary.toon` in `run-dir`. The summary is an `fh.run` document of the
//!    TOON schema (`toon-schema.md`, sections 3.5 and 3.8),
//! 4. exit with code 0 on success. On failure it exits with a non-zero code
//!    and prints the reason on stderr.
//!
//! ## Events
//!
//! Each line on stdout after the header is an event. A line that starts with
//! another word is a log line. The runner keeps all lines in `stdout.txt`.
//!
//! | Line | Meaning |
//! |---|---|
//! | `progress step=<i>/<n> phase=<name> fraction=<0..1>` | work done in the step |
//! | `estimate bytes=<n>` | the solver expects to need `n` bytes of memory |
//! | `restart <file>` | a restart marker in `run-dir`: the solver can continue from it |
//!
//! ## Cancel
//!
//! The runner writes the line `cancel` on the stdin of the solver. The solver
//! stops at the next safe point and exits with a non-zero code. The runner sets
//! [`CANCEL_ON_EOF`] to `1`. If stdin then closes, the solver must also stop,
//! because the runner is gone. Without the variable, the end of stdin does not
//! stop the solver, so one shell command with no input can solve a model. The
//! runner kills the solver after a grace period if it does not stop.
//!
//! ## Limits
//!
//! The runner starts the solver as the leader of its own process tree. It kills
//! the whole tree at the time limit or at the memory limit. The memory limit
//! counts the physical footprint on macOS and resident memory elsewhere.
//! Linux and Windows give the solver a hard limit that the kernel enforces.
//! If the runner cannot read the memory, it stops the solver and reports this.
//! The default memory limit is 85 % of the machine RAM.
//!
//! Results from a solver that did not pass the validation suite are marked
//! unvalidated.

mod pvd;
mod sys;

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use fh_inp::Model;

pub use sys::total_memory;

/// First line that a protocol v2 solver prints on stdout.
pub const PROTOCOL_HEADER: &str = "fh-solver-protocol 2";
pub const PVD_FILE: &str = "results.pvd";
/// The `fh.run` TOON summary.
pub const SUMMARY_FILE: &str = "summary.toon";
/// The runner puts this file in a run folder that it made. It clears only a
/// folder that has the file.
pub const RUN_MARKER: &str = ".fh-run";
/// Default time limit for a subprocess run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
/// Time that a solver has to stop after a cancel.
pub const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// The runner sets this environment variable to `1`. The solver then stops at the end of stdin.
pub const CANCEL_ON_EOF: &str = "FH_CANCEL_ON_EOF";

/// Stable diagnostic codes of the runner. `docs/diagnostics.md` lists them.
pub mod code {
    pub const CANCELLED: &str = "API001";
    pub const RUN_FOLDER: &str = "API002";
    pub const START_FAILED: &str = "API003";
    pub const PROTOCOL: &str = "API004";
    pub const MEMORY: &str = "API005";
    pub const TIME_LIMIT: &str = "API006";
    pub const SOLVER_FAILED: &str = "API007";
    pub const BAD_RESULT: &str = "API008";
    pub const PATH_OUTSIDE: &str = "API009";
    pub const SOLVER_OUTPUT: &str = "API010";
    pub const DUPLICATE_SOLVER: &str = "API011";
}

/// An error of the runner. It has a stable code and, when a file is the cause,
/// the name of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: &'static str,
    pub file: Option<String>,
    pub message: String,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}: ")?;
        }
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            code,
            file: None,
            message: message.into(),
        }
    }

    /// The same error with the name of the file that is the cause.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// An error that a solver reports about its own output or input.
    pub fn solver_output(message: impl Into<String>) -> Self {
        ApiError::new(code::SOLVER_OUTPUT, message)
    }

    /// The error of a run that the user cancelled.
    pub fn cancelled() -> Self {
        ApiError::new(code::CANCELLED, "run cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        self.code == code::CANCELLED
    }
}

fn fail<T>(code: &'static str, message: impl Into<String>) -> Result<T, ApiError> {
    Err(ApiError::new(code, message))
}

/// A result file of the solver as text. A file larger than `MAX_RESULT_FILE` is an error.
fn read_result(path: &Path, name: &str) -> Result<String, ApiError> {
    let bad = |message: String| ApiError::new(code::BAD_RESULT, message).with_file(name);
    let err = |e: std::io::Error| bad(e.to_string());
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(err)?
        .take(MAX_RESULT_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() as u64 > MAX_RESULT_FILE {
        return Err(bad(format!("larger than {MAX_RESULT_FILE} bytes")));
    }
    String::from_utf8(bytes).map_err(|_| bad("not UTF-8 text".into()))
}

/// The last `STDERR_TAIL` bytes of the solver's stderr. A failing solver writes the cause last.
fn stderr_tail(path: &Path) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    let mut bytes = Vec::new();
    if file
        .seek(SeekFrom::Start(len.saturating_sub(STDERR_TAIL)))
        .is_ok()
    {
        let _ = file.take(STDERR_TAIL).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What a solver run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub pvd: PathBuf,
    /// The text of `summary.toon`.
    pub summary: String,
    /// False until the solver passes the validation suite.
    pub validated: bool,
    /// The last restart marker that the solver reported, if any.
    pub restart: Option<PathBuf>,
}

/// An event line from a solver.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Progress {
        step: u32,
        steps: u32,
        phase: String,
        fraction: f64,
    },
    Estimate {
        bytes: u64,
    },
    Restart(String),
    Log(String),
}

impl Event {
    /// Read one line after the header. A line that does not fit is a log line.
    pub fn parse(line: &str) -> Event {
        let log = || Event::Log(line.to_owned());
        let mut words = line.split_whitespace();
        let key_value =
            |w: &str, key: &str| w.strip_prefix(key)?.strip_prefix('=').map(str::to_owned);
        match words.next() {
            Some("progress") => {
                let (mut step, mut phase, mut fraction) = (None, None, None);
                for w in words {
                    step = step.or_else(|| key_value(w, "step"));
                    phase = phase.or_else(|| key_value(w, "phase"));
                    fraction = fraction.or_else(|| key_value(w, "fraction"));
                }
                let parts = step.as_deref().and_then(|s| s.split_once('/'));
                match (parts, phase, fraction.and_then(|f| f.parse::<f64>().ok())) {
                    (Some((i, n)), Some(phase), Some(fraction)) => match (i.parse(), n.parse()) {
                        // Steps count from 1. The fraction is from 0 to 1. NaN is not in it.
                        (Ok(step), Ok(steps))
                            if (1..=steps).contains(&step) && (0.0..=1.0).contains(&fraction) =>
                        {
                            Event::Progress {
                                step,
                                steps,
                                phase,
                                fraction,
                            }
                        }
                        _ => log(),
                    },
                    _ => log(),
                }
            }
            Some("estimate") => words
                .find_map(|w| key_value(w, "bytes"))
                .and_then(|b| b.parse().ok())
                .map_or_else(log, |bytes| Event::Estimate { bytes }),
            Some("restart") => words
                .next()
                .map_or_else(log, |f| Event::Restart(f.to_owned())),
            _ => log(),
        }
    }
}

/// A flag that asks a running solver to stop. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// For a solver executable: read stdin in a thread, and set the returned flag at a `cancel`
/// line, or at the end of stdin when [`CANCEL_ON_EOF`] is `1`.
pub fn watch_stdin() -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let on_eof = std::env::var(CANCEL_ON_EOF).is_ok_and(|v| v == "1");
    std::thread::spawn(move || {
        // Read in blocks and keep only the start of each line, so a stream with no
        // line ends (for example /dev/zero) cannot use memory without limit.
        let mut stdin = std::io::stdin().lock();
        let (mut block, mut line) = ([0u8; 4096], Vec::<u8>::new());
        while let Ok(n @ 1..) = stdin.read(&mut block) {
            for &b in &block[..n] {
                if b == b'\n' {
                    if line.trim_ascii() == b"cancel" {
                        flag.store(true, Ordering::Relaxed);
                        return;
                    }
                    line.clear();
                } else if line.len() < 64 {
                    line.push(b);
                }
            }
        }
        if on_eof {
            flag.store(true, Ordering::Relaxed);
        }
    });
    cancel
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
            return fail(
                code::DUPLICATE_SOLVER,
                format!("solver {name} is already registered"),
            );
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

/// 85 % of the machine RAM. If the system does not tell the RAM, there is no limit.
pub fn default_memory_limit() -> u64 {
    total_memory().map_or(u64::MAX, |ram| ram / 100 * 85)
}

/// A solver in another executable that speaks protocol v2.
#[derive(Debug, Clone)]
pub struct SubprocessSolver {
    pub exe: PathBuf,
    pub timeout: Duration,
    /// Limit for the solver tree in bytes: physical footprint on macOS, resident elsewhere.
    /// `u64::MAX` means no limit.
    pub memory_limit: u64,
    pub cancel_grace: Duration,
    pub validated: bool,
    /// Reads the memory of a process tree. Tests replace it.
    pub probe: fn(u32) -> Result<u64, String>,
}

const POLL: Duration = Duration::from_millis(10);
const PROBE_EVERY: Duration = Duration::from_millis(100);
/// The longest stdout line that the runner reads, and the number of lines that can wait for
/// the run loop. Together they limit the runner memory for solver output to about 16 MiB.
/// The memory probe does not count the runner. Provisional values.
const MAX_LINE: usize = 64 << 10;
const BACKLOG: usize = 256;
/// The largest summary or PVD file that the runner reads, and the end of `stderr.txt` that
/// an error shows. A solver can write files of any size. Provisional values.
const MAX_RESULT_FILE: u64 = 16 << 20;
const STDERR_TAIL: u64 = 64 << 10;

impl SubprocessSolver {
    /// A new subprocess solver. It is unvalidated until the suite passes. The
    /// time limit is provisional. The memory limit is 85 % of the machine RAM.
    pub fn new(exe: impl Into<PathBuf>) -> Self {
        SubprocessSolver {
            exe: exe.into(),
            timeout: DEFAULT_TIMEOUT,
            memory_limit: default_memory_limit(),
            cancel_grace: CANCEL_GRACE,
            validated: false,
            probe: sys::default_probe(),
        }
    }

    /// Run the solver on `inp` in the folder `run_dir`. The runner clears the
    /// folder first, if it made the folder earlier.
    pub fn run(&self, inp: &Path, run_dir: &Path) -> Result<RunReport, ApiError> {
        self.run_with(inp, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Run and report events. `cancel` asks the solver to stop.
    pub fn run_with(
        &self,
        inp: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_event: &mut dyn FnMut(&Event),
    ) -> Result<RunReport, ApiError> {
        self.execute(inp, run_dir, false, cancel, on_event)
    }

    /// Continue an earlier run in `run_dir` from its restart marker.
    pub fn resume_with(
        &self,
        inp: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_event: &mut dyn FnMut(&Event),
    ) -> Result<RunReport, ApiError> {
        self.execute(inp, run_dir, true, cancel, on_event)
    }

    fn execute(
        &self,
        inp: &Path,
        run_dir: &Path,
        restart: bool,
        cancel: &CancelToken,
        on_event: &mut dyn FnMut(&Event),
    ) -> Result<RunReport, ApiError> {
        // A token that is cancelled before the start must stop the run, even for a model that
        // would finish before the first poll.
        if cancel.is_cancelled() {
            return Err(ApiError::cancelled());
        }
        // Check the input before the folder changes: a new run clears the folder.
        let inp = fs::canonicalize(inp).map_err(|e| {
            ApiError::new(code::START_FAILED, format!("input file: {e}"))
                .with_file(inp.display().to_string())
        })?;
        // The solver starts in the run folder. A relative path with a folder must resolve from
        // the folder of the caller. A bare name stays a name for the PATH search.
        let cannot_start = |e: &dyn std::fmt::Display| {
            ApiError::new(
                code::START_FAILED,
                format!("cannot start {}: {e}", self.exe.display()),
            )
        };
        let exe = if self.exe.is_relative() && self.exe.components().count() > 1 {
            std::path::absolute(&self.exe).map_err(|e| cannot_start(&e))?
        } else {
            self.exe.clone()
        };
        // A path that names no file cannot start. Check it before the folder is cleared.
        if exe.components().count() > 1 && !exe.is_file() {
            return Err(cannot_start(&"no such file"));
        }
        let run_dir = prepare_run_dir(run_dir, restart, &inp)?;
        let open = |name: &str| {
            fs::File::create(run_dir.join(name))
                .map_err(|e| ApiError::new(code::RUN_FOLDER, e.to_string()).with_file(name))
        };
        let limited = self.memory_limit != u64::MAX;
        let mut cmd = Command::new(&exe);
        cmd.arg(&inp).arg(&run_dir);
        if restart {
            cmd.arg("--restart");
        }
        cmd.current_dir(&run_dir)
            .env(CANCEL_ON_EOF, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(open("stderr.txt")?);
        let log = open("stdout.txt")?;
        sys::prepare(&mut cmd, limited.then_some(self.memory_limit));
        let child = cmd.spawn().map_err(|e| cannot_start(&e))?;
        // From here each return and each panic drops `tree`, and the drop kills the solver.
        let mut tree = Tree {
            group: sys::attach(&child, limited.then_some(self.memory_limit)),
            child,
        };
        let mut stdin = tree.child.stdin.take();
        let stdout = tree
            .child
            .stdout
            .take()
            .ok_or_else(|| ApiError::new(code::START_FAILED, "no stdout pipe"))?;
        // A full channel stops the reader, and then the pipe stops the solver.
        let (tx, rx) = mpsc::sync_channel::<Result<String, ApiError>>(BACKLOG);
        let reader = std::thread::spawn(move || {
            let mut log = log;
            let mut out = BufReader::new(stdout);
            loop {
                let mut buf = Vec::new();
                match (&mut out)
                    .take(MAX_LINE as u64 + 1)
                    .read_until(b'\n', &mut buf)
                {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                } else if buf.len() > MAX_LINE {
                    let _ = tx.send(Err(ApiError::new(
                        code::PROTOCOL,
                        format!("the solver wrote a stdout line longer than {MAX_LINE} bytes"),
                    )));
                    break;
                }
                // A line that is not UTF-8 is not an event. It stays as a log line.
                let line = String::from_utf8(buf)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                let _ = writeln!(log, "{line}");
                if tx.send(Ok(line)).is_err() {
                    break;
                }
            }
        });

        let start = Instant::now();
        let mut header_seen = false;
        let mut restart_file = None;
        let mut cancel_sent: Option<Instant> = None;
        let mut last_probe = Instant::now();
        let status = loop {
            // Events first, so that the header check comes before everything. Take events
            // for one poll period at most: a solver that writes without a pause, or a slow
            // callback, must not hold back the checks below.
            let batch = Instant::now();
            while let Ok(line) = rx.recv_timeout(POLL.saturating_sub(batch.elapsed())) {
                let line = match line {
                    Ok(line) => line,
                    Err(e) => return stop(&mut tree, e),
                };
                if !header_seen {
                    if line != PROTOCOL_HEADER {
                        return stop(&mut tree, ApiError::new(code::PROTOCOL, format!(
                            "solver does not speak protocol v2: first stdout line is not \"{PROTOCOL_HEADER}\""
                        )));
                    }
                    header_seen = true;
                    continue;
                }
                let event = Event::parse(&line);
                if let Event::Estimate { bytes } = event {
                    if limited && bytes > self.memory_limit {
                        return stop(&mut tree, ApiError::new(code::MEMORY, format!(
                            "solver expects to need {bytes} bytes, more than the memory limit of {} bytes",
                            self.memory_limit
                        )));
                    }
                }
                if let Event::Restart(f) = &event {
                    restart_file = Some(f.clone());
                }
                on_event(&event);
                if batch.elapsed() >= POLL {
                    break;
                }
            }
            if cancel.is_cancelled() && cancel_sent.is_none() {
                if let Some(mut pipe) = stdin.take() {
                    let _ = writeln!(pipe, "cancel").and_then(|()| pipe.flush());
                    stdin = Some(pipe);
                }
                cancel_sent = Some(Instant::now());
            }
            if cancel_sent.is_some_and(|t| t.elapsed() > self.cancel_grace) {
                return stop(&mut tree, ApiError::cancelled());
            }
            if start.elapsed() > self.timeout {
                return stop(
                    &mut tree,
                    ApiError::new(
                        code::TIME_LIMIT,
                        format!(
                            "solver stopped after the time limit of {} s",
                            self.timeout.as_secs_f64()
                        ),
                    ),
                );
            }
            if limited && last_probe.elapsed() >= PROBE_EVERY {
                last_probe = Instant::now();
                match (self.probe)(tree.group.pid()) {
                    Ok(b) if b > self.memory_limit => {
                        return stop(
                            &mut tree,
                            ApiError::new(
                                code::MEMORY,
                                format!(
                            "solver stopped after it used more than the memory limit of {} bytes",
                            self.memory_limit
                        ),
                            ),
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        // The solver can end before the probe.
                        if let Ok(Some(s)) = tree.child.try_wait() {
                            break s;
                        }
                        return stop(&mut tree, ApiError::new(code::MEMORY, format!(
                            "the memory probe failed ({e}), so the runner cannot enforce the memory limit"
                        )));
                    }
                }
            }
            // The end of the solver comes after the checks, so a late end does not hide a
            // cancel or the time limit.
            match tree.child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) => {}
                Err(e) => {
                    return stop(
                        &mut tree,
                        ApiError::new(code::SOLVER_FAILED, format!("wait failed: {e}")),
                    )
                }
            }
        };
        // The solver has ended. Remove the processes that it left behind, then
        // read the last events. A stray process can hold the pipe, so do not wait for it.
        tree.group.kill();
        drop(stdin);
        while let Ok(line) = rx.recv_timeout(Duration::from_millis(200)) {
            let line = line?;
            if !header_seen {
                // The first line decides, as in the loop above. After a wrong first line
                // the other lines are not events, and the checks below give the error.
                if line != PROTOCOL_HEADER {
                    break;
                }
                header_seen = true;
                continue;
            }
            let event = Event::parse(&line);
            if let Event::Restart(f) = &event {
                restart_file = Some(f.clone());
            }
            on_event(&event);
            // The backlog is short, but a process outside the tree can keep the pipe and
            // write without end. The time limit and a cancel also stop this read.
            if cancel.is_cancelled() {
                break;
            }
            if start.elapsed() > self.timeout {
                return fail(
                    code::TIME_LIMIT,
                    format!(
                        "solver stopped after the time limit of {} s",
                        self.timeout.as_secs_f64()
                    ),
                );
            }
        }
        drop(reader);
        // A cancel stops the run, also when the solver exits with code 0, or when the cancel
        // comes as the solver ends.
        if cancel_sent.is_some() || cancel.is_cancelled() {
            return Err(ApiError::cancelled());
        }
        if !status.success() {
            return fail(
                code::SOLVER_FAILED,
                format!(
                    "solver failed ({status}): {}",
                    stderr_tail(&run_dir.join("stderr.txt")).trim()
                ),
            );
        }
        if !header_seen {
            return fail(
                code::PROTOCOL,
                format!(
                "solver does not speak protocol v2: first stdout line is not \"{PROTOCOL_HEADER}\""
            ),
            );
        }
        let summary = read_result(&run_dir.join(SUMMARY_FILE), SUMMARY_FILE)?;
        check_summary(&summary)?;
        let pvd = run_dir.join(PVD_FILE);
        check_pvd(&run_dir, &pvd)?;
        let restart = restart_file
            .map(|f| inside(&run_dir, &f))
            .transpose()?
            .filter(|p| p.is_file());
        Ok(RunReport {
            pvd,
            summary,
            validated: self.validated,
            restart,
        })
    }
}

/// The process tree of a running solver. A drop kills it, so that an early return or a
/// panic in an event callback cannot leave the solver running without its limits.
struct Tree {
    child: std::process::Child,
    group: sys::Group,
}

impl Drop for Tree {
    fn drop(&mut self) {
        // After a normal end the runner has already killed the group.
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            self.group.kill();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Kill the whole tree of the solver and return `reason`.
fn stop(tree: &mut Tree, reason: ApiError) -> Result<RunReport, ApiError> {
    tree.group.kill();
    let _ = tree.child.kill();
    let _ = tree.child.wait();
    Err(reason)
}

/// Make the run folder ready. A folder with files in it must be a run folder of
/// this runner (it has the marker file), or the call fails. A fresh run clears it,
/// so the input `inp` (a real path) must be outside it.
fn prepare_run_dir(run_dir: &Path, restart: bool, inp: &Path) -> Result<PathBuf, ApiError> {
    let err = |e: std::io::Error| ApiError::new(code::RUN_FOLDER, format!("run folder: {e}"));
    fs::create_dir_all(run_dir).map_err(err)?;
    let dir = fs::canonicalize(run_dir).map_err(err)?;
    if !restart && inp.starts_with(&dir) {
        return fail(
            code::RUN_FOLDER,
            format!(
                "input file {} is inside the run folder, which a new run clears",
                inp.display()
            ),
        );
    }
    let marked = dir.join(RUN_MARKER).is_file();
    let empty = fs::read_dir(&dir).map_err(err)?.next().is_none();
    if restart {
        if !marked {
            return fail(code::RUN_FOLDER, "run folder has no earlier run to restart");
        }
        // The summary marks a complete run. Only the restart can write a new one, so an old
        // summary cannot pass as the result of the restart. The other files stay for the solver.
        match fs::remove_file(dir.join(SUMMARY_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(e)),
            _ => {}
        }
        return Ok(dir);
    }
    if !empty && !marked {
        return fail(
            code::RUN_FOLDER,
            format!(
            "run folder {} has files and is not a run folder of femahoy: choose an empty folder",
            dir.display()
        ),
        );
    }
    for entry in fs::read_dir(&dir).map_err(err)?.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            fs::remove_dir_all(&path).map_err(err)?;
        } else {
            fs::remove_file(&path).map_err(err)?;
        }
    }
    fs::write(dir.join(RUN_MARKER), "femahoy run folder\n").map_err(err)?;
    Ok(dir)
}

/// The path `name` in `run_dir`, if it stays inside. A name must be relative, have
/// no `..` and name a file whose real path is inside the folder.
fn inside(run_dir: &Path, name: &str) -> Result<PathBuf, ApiError> {
    let rel = Path::new(name);
    if rel.is_absolute() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        return fail(
            code::PATH_OUTSIDE,
            format!("{name} is not a path inside the run folder"),
        );
    }
    let path = run_dir.join(rel);
    match fs::canonicalize(&path) {
        Ok(real) if real.starts_with(run_dir) => Ok(real),
        Ok(_) => fail(
            code::PATH_OUTSIDE,
            format!("{name} leads out of the run folder"),
        ),
        Err(_) => Ok(path),
    }
}

fn check_summary(text: &str) -> Result<(), ApiError> {
    let first = text.lines().next().unwrap_or_default();
    if first != "schema: fh.run" || !text.lines().any(|l| l.starts_with("status: ")) {
        return Err(ApiError::new(
            code::BAD_RESULT,
            "not an fh.run document of the TOON schema",
        )
        .with_file(SUMMARY_FILE));
    }
    Ok(())
}

/// Check the PVD file. One error lists every VTU file that is missing.
fn check_pvd(run_dir: &Path, path: &Path) -> Result<(), ApiError> {
    let text = read_result(path, PVD_FILE)?;
    let bad = |message: String| ApiError::new(code::BAD_RESULT, message).with_file(PVD_FILE);
    let listed =
        pvd::data_set_files(&text).map_err(|e| bad(format!("not a PVD collection: {e}")))?;
    if listed.is_empty() {
        return Err(bad("lists no VTU file".into()));
    }
    let mut missing = Vec::new();
    for f in listed {
        if !inside(run_dir, &f)?.is_file() {
            missing.push(format!("lists {f}, but the file is missing"));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(bad(missing.join("; ")))
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn errors_have_a_code_and_a_file() {
        let e = check_summary("junk").unwrap_err();
        assert_eq!(e.code, code::BAD_RESULT);
        assert_eq!(e.file.as_deref(), Some(SUMMARY_FILE));
        assert_eq!(
            e.to_string(),
            "summary.toon: not an fh.run document of the TOON schema (API008)"
        );
        assert!(ApiError::cancelled().is_cancelled());
    }

    #[test]
    fn pvd_check_lists_every_missing_file() {
        let dir = std::env::temp_dir().join(format!("fh-pvd-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let pvd = dir.join(PVD_FILE);
        fs::write(
            &pvd,
            r#"<VTKFile type="Collection"><Collection><DataSet timestep="0" file="a.vtu"/><DataSet timestep="1" file="b.vtu"/></Collection></VTKFile>"#,
        )
        .unwrap();
        let e = check_pvd(&dir, &pvd).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            e.message.contains("a.vtu") && e.message.contains("b.vtu"),
            "{e}"
        );
        assert_eq!(e.file.as_deref(), Some(PVD_FILE));
    }
}
