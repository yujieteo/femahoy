//! CalculiX (`ccx`) as the solver of femahoy, in the process of `fh` (D-13, see DESIGN.md).
//!
//! [`Ccx::run`] gives the user's `ccx` (`$FH_CCX`, else `ccx` on PATH) the input file as it is,
//! with each `*INCLUDE` file, in `<run-dir>/ccx`. Then it converts every nodal field of the
//! `.frd` file into one VTU file for each step, and writes `results.pvd` in the run folder.
//! `ccx` is GPL, so femahoy only starts it as a separate program.
//!
//! # Limits
//!
//! `ccx` leads its own process group. The run kills the whole group at a cancel, at the
//! time limit or at the memory limit. The memory limit counts the physical footprint on
//! macOS and the resident memory elsewhere. Linux also gives `ccx` a hard limit that the
//! kernel enforces. If the memory cannot be read, the run stops `ccx` and tells this. The
//! default memory limit is 85 % of the machine RAM. There is no default time limit.
//!
//! A guard process in the group of `ccx` reads a pipe from this process. When this process
//! ends, also on SIGKILL, the pipe closes and the guard kills the group. Thus `ccx` never
//! lives longer than `fh`.

mod frd;
mod sys;

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fh_io::vtu::{write_pvd_units, write_vtu, PvdEntry};

pub use sys::total_memory;

pub const PVD_FILE: &str = "results.pvd";
/// A run puts this file in a run folder that it made. It clears only a folder that has the
/// file.
pub const RUN_MARKER: &str = ".fh-run";
/// Name of the `ccx` job in `<run-dir>/ccx`.
const JOB: &str = "job";

/// Diagnostic codes of a run.
pub mod code {
    pub const CANCELLED: &str = "API001";
    pub const RUN_FOLDER: &str = "API002";
    pub const START_FAILED: &str = "API003";
    pub const MEMORY: &str = "API005";
    pub const TIME_LIMIT: &str = "API006";
    pub const SOLVER_FAILED: &str = "API007";
}

/// An error of a run, with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Error {
            code,
            message: message.into(),
        }
    }

    /// The error of a run that the user cancelled.
    pub fn cancelled() -> Self {
        Error::new(code::CANCELLED, "run cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        self.code == code::CANCELLED
    }
}

/// An error of the model or of the `ccx` files.
fn failed(message: impl Into<String>) -> Error {
    Error::new(code::SOLVER_FAILED, message)
}

/// Copy `inp` to `dir/name` and each file of `files` (paths relative to the folder of `inp`) to
/// the same path in `dir`. Each copy is read-only, so that a later edit cannot change a run.
fn copy_files(inp: &Path, files: &[PathBuf], dir: &Path, name: &str) -> std::io::Result<()> {
    let from = inp.parent().unwrap_or(Path::new("."));
    let pairs = std::iter::once((inp.to_owned(), dir.join(name)))
        .chain(files.iter().map(|f| (from.join(f), dir.join(f))));
    for (src, dst) in pairs {
        if dst.exists() {
            continue;
        }
        fs::create_dir_all(dst.parent().unwrap_or(dir))?;
        fs::copy(&src, &dst)?;
        let mut perms = fs::metadata(&dst)?.permissions();
        perms.set_readonly(true);
        fs::set_permissions(&dst, perms)?;
    }
    Ok(())
}

/// Copy the input file `inp` to `dir/name`, and each file that it includes to its relative path
/// in `dir`. Each copy is read-only. A later run of `dir/name` does not change when `inp` does.
pub fn stage_input(inp: &Path, dir: &Path, name: &str) -> Result<(), Error> {
    let flat = fh_inp::flatten(inp).map_err(failed)?;
    copy_files(inp, &flat.files, dir, name)
        .map_err(|e| Error::new(code::RUN_FOLDER, format!("input copy: {e}")))
}

/// The text of an input file, with the text of each `*INCLUDE` file in the place of its line.
pub fn input_text(inp: &Path) -> Result<String, Error> {
    fh_inp::flatten(inp).map(|f| f.text).map_err(failed)
}

/// A flag that asks a running solve to stop. Clones share the flag.
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

/// 85 % of the machine RAM. If the system does not tell the RAM, there is no limit.
pub fn default_memory_limit() -> u64 {
    total_memory().map_or(u64::MAX, |ram| ram / 100 * 85)
}

/// Why `ccx` failed and what to do, from its exit status and its output `log`.
fn ccx_failure(status: ExitStatus, log: &str, log_file: &Path) -> String {
    let see = format!("The full ccx output is in {}", log_file.display());
    // The first `*ERROR` message: its line and the lines after it, up to an empty line.
    let mut lines = log.lines().skip_while(|l| !l.contains("*ERROR"));
    if let Some(first) = lines.next() {
        let rest = lines.take_while(|l| !l.trim().is_empty() && !l.contains("*ERROR"));
        let words: Vec<&str> = std::iter::once(first)
            .chain(rest)
            .flat_map(str::split_whitespace)
            .collect();
        return format!(
            "ccx failed ({status}): {}. Correct the model as the message tells. {see}",
            words.join(" ")
        );
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return format!(
            "ccx stopped on signal {signal} and gave no error message. Usually the cause is \
             too little memory for the model. Use a coarser mesh, or a computer with more \
             memory. {see}"
        );
    }
    let last = log.lines().map(str::trim).rfind(|l| !l.is_empty());
    format!(
        "ccx failed ({status}) with no *ERROR message. The last ccx line is: {}. {see}",
        last.unwrap_or("(no output)")
    )
}

/// Write the VTU file of each step and the PVD collection in `units`. Each file is written
/// under a temporary name and renamed (D-09).
fn write_results(results: &frd::Frd, run_dir: &Path, units: &str) -> Result<(), String> {
    let atomic = |file: &str, bytes: &[u8]| -> Result<(), String> {
        let tmp = run_dir.join(format!("{file}.tmp"));
        fs::write(&tmp, bytes)
            .and_then(|()| fs::rename(&tmp, run_dir.join(file)))
            .map_err(|e| format!("{file}: {e}"))
    };
    let mut entries = Vec::new();
    for (i, frame) in results.frames.iter().enumerate() {
        let file = format!("step-{}.vtu", i + 1);
        let bytes = write_vtu(&results.grid(frame)).map_err(|e| e.to_string())?;
        atomic(&file, &bytes)?;
        entries.push(PvdEntry {
            time: frame.time,
            file,
        });
    }
    atomic(PVD_FILE, write_pvd_units(&entries, units).as_bytes())
}

/// The place of a running solve, from the output of `ccx`. Steps and increments count from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub step: u64,
    pub increment: u64,
}

/// Reads the new lines of the log of `ccx` and tells the place of the solve.
struct LogTail {
    file: PathBuf,
    pos: u64,
    last: Option<Progress>,
}

impl LogTail {
    fn poll(&mut self, on_progress: &mut dyn FnMut(Progress)) {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = fs::File::open(&self.file) else {
            return;
        };
        let mut bytes = Vec::new();
        if f.seek(SeekFrom::Start(self.pos)).is_err() || f.read_to_end(&mut bytes).is_err() {
            return;
        }
        let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else {
            return;
        };
        self.pos += end as u64 + 1;
        for line in String::from_utf8_lossy(&bytes[..end]).lines() {
            let mut words = line.split_whitespace();
            let (key, n) = (
                words.next(),
                words.next().and_then(|n| n.parse::<u64>().ok()),
            );
            let now = match (key, n, self.last) {
                (Some("STEP"), Some(step), _) => Progress { step, increment: 0 },
                (Some("INCREMENT"), Some(increment), Some(p)) => Progress { increment, ..p },
                _ => continue,
            };
            if self.last != Some(now) {
                self.last = Some(now);
                if now.increment > 0 {
                    on_progress(now);
                }
            }
        }
    }
}

/// The `ccx` program with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Ccx {
    /// `$FH_CCX`, else `ccx` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit: a large model can solve for hours.
    pub timeout: Option<Duration>,
    /// Limit for the process group of `ccx` in bytes: physical footprint on macOS, resident
    /// elsewhere. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Ccx {
    fn default() -> Self {
        Self::new()
    }
}

const POLL: Duration = Duration::from_millis(10);
const PROBE_EVERY: Duration = Duration::from_millis(100);

/// What to do when a run does not fit in its limits.
const SMALLER: &str =
    "Make the model smaller, for example with a coarser mesh (a larger element size)";

impl Ccx {
    /// `ccx` from `$FH_CCX`, else `ccx` on PATH. It has no time limit. The memory limit is
    /// 85 % of the machine RAM.
    pub fn new() -> Self {
        Ccx {
            exe: std::env::var_os("FH_CCX").map_or_else(|| PathBuf::from("ccx"), PathBuf::from),
            timeout: None,
            memory_limit: default_memory_limit(),
        }
    }

    /// Solve `inp` in the folder `run_dir`. The run clears the folder first, if a run made
    /// the folder earlier.
    pub fn run(&self, inp: &Path, run_dir: &Path) -> Result<(), Error> {
        self.run_with(inp, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Solve, and stop when `cancel` is set. `on_progress` gets the step and the increment
    /// that `ccx` starts.
    pub fn run_with(
        &self,
        inp: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        // Check the input before the folder changes: a new run clears the folder.
        let inp = fs::canonicalize(inp).map_err(|e| {
            Error::new(
                code::START_FAILED,
                format!("{}: input file: {e}", inp.display()),
            )
        })?;
        let flat = fh_inp::flatten(&inp).map_err(failed)?;
        let run_dir = prepare_run_dir(run_dir, &inp)?;
        let work = run_dir.join("ccx");
        let folder = |e: std::io::Error| Error::new(code::RUN_FOLDER, format!("run folder: {e}"));
        fs::create_dir_all(&work).map_err(folder)?;
        // `ccx` gets the file unchanged. An `*INCLUDE` path is relative to the folder of `ccx`.
        copy_files(&inp, &flat.files, &work, &format!("{JOB}.inp")).map_err(folder)?;
        let log_file = work.join(format!("{JOB}.log"));
        let log = fs::File::create(&log_file).map_err(folder)?;
        let status = self.solve(&work, &log_file, log, cancel, on_progress)?;
        let log = fs::read_to_string(&log_file).unwrap_or_default();
        if !status.success() || log.contains("*ERROR") {
            return Err(failed(ccx_failure(status, &log, &log_file)));
        }
        let file = work.join(format!("{JOB}.frd"));
        let bytes = fs::read(&file).map_err(|e| failed(format!("{JOB}.frd: {e}")))?;
        let results = frd::read(&String::from_utf8_lossy(&bytes)).map_err(failed)?;
        let units = fh_inp::units(&flat.text);
        let units = if matches!(units.as_str(), "SI-mm" | "SI-m") {
            units.as_str()
        } else {
            "unspecified"
        };
        write_results(&results, &run_dir, units).map_err(failed)
    }

    /// Run `ccx -i job` in `work` with its output in `log`, in its own process group, to its
    /// end or to a stop.
    fn solve(
        &self,
        work: &Path,
        log_file: &Path,
        log: fs::File,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<ExitStatus, Error> {
        let cannot_start = |e: std::io::Error| {
            Error::new(
                code::START_FAILED,
                format!(
                    "cannot start {}: {e}. Install CalculiX (README.md) and put ccx on PATH, \
                     or set FH_CCX",
                    self.exe.display()
                ),
            )
        };
        let limited = self.memory_limit != u64::MAX;
        // The guard leads the group. It stops the group when the pipe from this process
        // closes. It is the leader, so the group exists until the end of `ccx`.
        let mut guard = Command::new("/bin/sh");
        guard
            .args(["-c", "while read -r _; do :; done; kill -KILL 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        sys::prepare(&mut guard, None, 0);
        let guard = guard.spawn().map_err(cannot_start)?;
        let group = sys::Group::new(guard.id());
        let mut tree = Tree {
            group,
            guard,
            ccx: None,
        };
        let mut cmd = Command::new(&self.exe);
        cmd.args(["-i", JOB])
            .current_dir(work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(cannot_start)?)
            .stderr(log);
        sys::prepare(
            &mut cmd,
            limited.then_some(self.memory_limit),
            tree.group.pid(),
        );
        // From here each return and each panic drops `tree`, and the drop kills the group.
        tree.ccx = Some(cmd.spawn().map_err(cannot_start)?);
        let start = Instant::now();
        let mut last_probe = Instant::now();
        let mut tail = LogTail {
            file: log_file.to_owned(),
            pos: 0,
            last: None,
        };
        let mut last_tail = Instant::now();
        loop {
            if last_tail.elapsed() >= PROBE_EVERY {
                last_tail = Instant::now();
                tail.poll(on_progress);
            }
            if cancel.is_cancelled() {
                return Err(Error::cancelled());
            }
            if let Some(t) = self.timeout.filter(|t| start.elapsed() > *t) {
                return Err(Error::new(
                    code::TIME_LIMIT,
                    format!(
                        "the solve stopped at the time limit of {} s. {SMALLER}",
                        t.as_secs_f64()
                    ),
                ));
            }
            if limited && last_probe.elapsed() >= PROBE_EVERY {
                last_probe = Instant::now();
                match sys::probe(tree.group.pid()) {
                    Ok(b) if b > self.memory_limit => {
                        return Err(Error::new(
                            code::MEMORY,
                            format!(
                                "the solve stopped after ccx used more than the memory limit \
                                 of {}. {SMALLER}",
                                size(self.memory_limit)
                            ),
                        ));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        return Err(Error::new(
                            code::MEMORY,
                            format!(
                                "the memory probe failed ({e}), so the memory limit cannot \
                                 apply"
                            ),
                        ));
                    }
                }
            }
            // The end of `ccx` comes after the checks, so a late end does not hide a cancel
            // or the time limit.
            let ccx = tree.ccx.as_mut().expect("ccx started");
            match ccx.try_wait() {
                Ok(Some(status)) => {
                    tail.poll(on_progress);
                    return Ok(status);
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => return Err(failed(format!("wait for ccx failed: {e}"))),
            }
        }
    }
}

/// The process group of a run: the guard and `ccx`. A drop kills the group, so that an early
/// return or a panic cannot leave `ccx` running without its limits.
struct Tree {
    group: sys::Group,
    guard: Child,
    ccx: Option<Child>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        // Also the processes that `ccx` left behind.
        self.group.kill();
        for child in std::iter::once(&mut self.guard).chain(&mut self.ccx) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A memory size for a person: GiB from 1 GiB, else MiB.
fn size(bytes: u64) -> String {
    let mib = bytes as f64 / f64::from(1 << 20);
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else {
        format!("{} MiB", (mib * 1000.0).round() / 1000.0)
    }
}

/// Make the run folder ready. A folder with files in it must be a run folder (it has the
/// marker file), or the call fails. A fresh run clears it, so the input `inp` (a real path)
/// must be outside it.
fn prepare_run_dir(run_dir: &Path, inp: &Path) -> Result<PathBuf, Error> {
    let fail = |message: String| Err(Error::new(code::RUN_FOLDER, message));
    let err = |e: std::io::Error| Error::new(code::RUN_FOLDER, format!("run folder: {e}"));
    fs::create_dir_all(run_dir).map_err(err)?;
    let dir = fs::canonicalize(run_dir).map_err(err)?;
    if inp.starts_with(&dir) {
        return fail(format!(
            "input file {} is inside the run folder, which a new run clears",
            inp.display()
        ));
    }
    let marked = dir.join(RUN_MARKER).is_file();
    let empty = fs::read_dir(&dir).map_err(err)?.next().is_none();
    if !empty && !marked {
        return fail(format!(
            "run folder {} has files and is not a run folder of femahoy: choose an empty folder",
            dir.display()
        ));
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
