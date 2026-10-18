//! The run service: the store on disk, the run queue and the event queues. The HTTP
//! transport and an in-process caller use the same commands.
//!
//! # Store
//!
//! ```text
//! <state>/daemon.lock                       exclusive lock of the one daemon
//! <state>/daemon.info                       port and token for clients (mode 0600)
//! <state>/acks/<request>                    acknowledgement of each command
//! <state>/projects/<project>/root           the project root folder
//! <state>/projects/<project>/revisions/<revision>/input.inp   fixed input, read-only
//! <state>/projects/<project>/runs/<run>/record                the run record
//! <state>/projects/<project>/runs/<run>/events.log            bounded event log
//! <state>/projects/<project>/runs/<run>/out/                  protocol v2 run folder
//! ```
//!
//! Each record and acknowledgement is written to a temporary file and then renamed, so a
//! reader never sees a partial file.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fh_solver_api::{ApiError, CancelToken, Event as SolverEvent, SubprocessSolver};

use crate::contract::{
    now_ms, parse_event, run_id, valid_project, valid_request, valid_revision, valid_run, Ack,
    Command, Error, Event, Events, Message, Outcome, RunRecord, RunState,
};
use crate::random_hex;

/// The most runs that can wait in the queue. A submit after that fails with `queue.full`.
pub const MAX_QUEUED: usize = 64;
/// The most events that a run keeps in memory. A client that reads too late gets `dropped`.
pub const EVENT_QUEUE: usize = 1024;
/// The longest event text in bytes. A longer text is cut.
pub const EVENT_TEXT: usize = 1024;
/// The largest event log of a run in bytes. State events are always written.
pub const EVENT_LOG: u64 = 1 << 20;
/// The most events in one answer.
pub const EVENT_BATCH: usize = 256;
/// The longest time that an events request waits for a new event.
pub const MAX_WAIT: Duration = Duration::from_secs(30);
/// The most acknowledgements that the daemon keeps. At start it removes the oldest.
pub const MAX_ACKS: usize = 4096;
/// The number of ended runs whose events stay in memory. The others come from their log.
const KEEP_ENDED: usize = 32;

pub(crate) const LOCK_FILE: &str = "daemon.lock";
const ROOT_FILE: &str = "root";
const RECORD_FILE: &str = "record";
const EVENTS_FILE: &str = "events.log";
const INPUT_FILE: &str = "input.inp";
const OUT_DIR: &str = "out";
const STOPPED: &str = "the daemon stopped before the run ended";

/// The solvers that a submit can name. The first one is the solver of [`Config`].
pub const SOLVERS: [&str; 2] = ["fh-solve", "fh-solve-ccx"];

/// The settings of a run service.
pub struct Config {
    /// The folder of the store. One daemon at a time uses it.
    pub state_dir: PathBuf,
    /// The protocol v2 solver, for example `fh-solve`, with its limits.
    pub solver: SubprocessSolver,
    /// The folders, in order, where a run finds another solver of [`SOLVERS`] that its
    /// submit names. `fh serve` gives [`solver_dirs`].
    pub solver_dirs: Vec<PathBuf>,
}

/// The folder of the running program (`fh`), then each folder of `PATH`.
pub fn solver_dirs() -> Vec<PathBuf> {
    let exe = std::env::current_exe().ok();
    let path = std::env::var_os("PATH").unwrap_or_default();
    exe.as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .into_iter()
        .chain(std::env::split_paths(&path))
        .collect()
}

/// The program `name` in the first folder of `dirs` that has it.
pub fn find_solver(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    dirs.iter().map(|d| d.join(&file)).find(|p| p.is_file())
}

/// The message for a solver that [`find_solver`] does not find.
pub fn missing_solver(name: &str) -> String {
    format!(
        "the solver {name} is not beside fh or on PATH. To install it, see docs/ccx.md \
         (run.solver.missing)"
    )
}

/// The run service. It owns every run. Clients only send commands and read records.
pub struct Service {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
    lock: Mutex<Option<File>>,
}

struct Shared {
    dir: PathBuf,
    instance: String,
    solver: SubprocessSolver,
    solver_dirs: Vec<PathBuf>,
    inner: Mutex<Inner>,
    /// Wakes the worker for a new run, and the readers for a new event.
    wake: Condvar,
}

type Key = (String, String);

#[derive(Default)]
struct Inner {
    projects: BTreeMap<String, PathBuf>,
    next_run: BTreeMap<String, u64>,
    /// The runs of this daemon instance, with their event queues.
    live: BTreeMap<Key, Live>,
    queue: VecDeque<Key>,
    ended: VecDeque<Key>,
    stopping: bool,
}

struct Live {
    record: RunRecord,
    events: EventQueue,
    log: Option<File>,
    log_bytes: u64,
    log_full: bool,
    cancel: CancelToken,
}

impl Live {
    fn push(&mut self, text: &str) {
        let text = clip(text, EVENT_TEXT);
        let seq = self.events.push(text.clone());
        let Some(log) = &mut self.log else { return };
        let line = format!("{seq} {text}\n");
        if text.starts_with("state ") || self.log_bytes + line.len() as u64 <= EVENT_LOG {
            if log.write_all(line.as_bytes()).is_ok() {
                self.log_bytes += line.len() as u64;
            }
        } else if !self.log_full {
            self.log_full = true;
            let _ = writeln!(
                log,
                "{seq} log-full the event log reached {EVENT_LOG} bytes"
            );
        }
    }
}

/// A bounded queue of events with sequence numbers.
struct EventQueue {
    items: VecDeque<Event>,
    next: u64,
}

impl EventQueue {
    fn new() -> Self {
        EventQueue {
            items: VecDeque::new(),
            next: 1,
        }
    }

    fn push(&mut self, text: String) -> u64 {
        let seq = self.next;
        self.add(Event { seq, text });
        seq
    }

    fn add(&mut self, event: Event) {
        if self.items.len() == EVENT_QUEUE {
            self.items.pop_front();
        }
        self.next = event.seq + 1;
        self.items.push_back(event);
    }

    fn since(&self, since: u64, state: RunState) -> Events {
        let first = self.items.front().map_or(self.next, |e| e.seq);
        Events {
            items: self
                .items
                .iter()
                .filter(|e| e.seq > since)
                .take(EVENT_BATCH)
                .cloned()
                .collect(),
            last: self.next - 1,
            dropped: first.saturating_sub(since.saturating_add(1)),
            state,
        }
    }

    /// The last events of an event log.
    fn load(path: &Path) -> Result<EventQueue, Error> {
        let mut queue = EventQueue::new();
        let mut bytes = Vec::new();
        match File::open(path) {
            Ok(f) => f
                .take(2 * EVENT_LOG)
                .read_to_end(&mut bytes)
                .map_err(|e| Error::io("event log", e))?,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(queue),
            Err(e) => return Err(Error::io("event log", e)),
        };
        for event in String::from_utf8_lossy(&bytes)
            .lines()
            .filter_map(parse_event)
        {
            queue.add(event);
        }
        Ok(queue)
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn solver_text(event: &SolverEvent) -> String {
    match event {
        SolverEvent::Progress {
            step,
            steps,
            phase,
            fraction,
        } => format!("progress step={step}/{steps} phase={phase} fraction={fraction}"),
        SolverEvent::Estimate { bytes } => format!("estimate bytes={bytes}"),
        SolverEvent::Restart(file) => format!("restart {file}"),
        SolverEvent::Log(line) => format!("log {line}"),
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), Error> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    let err = |e| Error::io(&path.display().to_string(), e);
    let mut file = File::create(&tmp).map_err(err)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(err)?;
    fs::rename(&tmp, path).map_err(err)
}

fn read_message(path: &Path) -> Result<Option<Message>, Error> {
    match fs::read_to_string(path) {
        Ok(text) => Message::parse(&text).map(Some),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(&path.display().to_string(), e)),
    }
}

/// FNV-1a, as the `source_hash` of the `fh.run` summary, so the two agree.
fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Copy `src` to `dst` and return the hash of the bytes.
fn copy_hashed(src: &Path, dst: &Path) -> Result<u64, Error> {
    let err = |e| Error::io("input copy", e);
    let mut from = File::open(src).map_err(err)?;
    let mut to = File::create(dst).map_err(err)?;
    let mut buf = vec![0; 64 << 10];
    let mut hash = 0xcbf2_9ce4_8422_2325;
    loop {
        let n = from.read(&mut buf).map_err(err)?;
        if n == 0 {
            break;
        }
        hash = fnv(hash, &buf[..n]);
        to.write_all(&buf[..n]).map_err(err)?;
    }
    to.sync_all().map_err(err)?;
    Ok(hash)
}

fn same_bytes(a: &Path, b: &Path) -> Result<bool, Error> {
    let err = |e| Error::io("revision compare", e);
    let (mut fa, mut fb) = (File::open(a).map_err(err)?, File::open(b).map_err(err)?);
    if fa.metadata().map_err(err)?.len() != fb.metadata().map_err(err)?.len() {
        return Ok(false);
    }
    let (mut ba, mut bb) = (vec![0; 64 << 10], vec![0; 64 << 10]);
    loop {
        let n = fa.read(&mut ba).map_err(err)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n]).map_err(err)?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn project_dir(&self, project: &str) -> PathBuf {
        self.dir.join("projects").join(project)
    }

    fn run_dir(&self, project: &str, run: &str) -> PathBuf {
        self.project_dir(project).join("runs").join(run)
    }

    fn ack_path(&self, request: &str) -> PathBuf {
        self.dir.join("acks").join(request)
    }

    fn save(&self, record: &RunRecord) -> Result<(), Error> {
        let path = self.run_dir(&record.project, &record.run).join(RECORD_FILE);
        write_atomic(&path, &record.to_message().to_text())
    }

    /// Move a live run to `state`, keep the record and add a state event.
    fn change(&self, live: &mut Live, state: RunState, message: &str) {
        debug_assert!(live.record.state.can_move_to(state));
        live.record.state = state;
        if state == RunState::Running {
            live.record.started_ms = now_ms();
        }
        if state.is_final() {
            live.record.finished_ms = now_ms();
        }
        if !message.is_empty() {
            live.record.message = message.to_owned();
        }
        live.push(&format!("state {state}"));
        // A record that cannot be written keeps its earlier state on disk. The next daemon
        // start then marks the run interrupted, so the record never shows a false success.
        let _ = self.save(&live.record);
    }

    fn finish(&self, inner: &mut Inner, key: &Key, state: RunState, message: &str) {
        if let Some(live) = inner.live.get_mut(key) {
            self.change(live, state, message);
            live.log = None;
        }
        inner.ended.push_back(key.clone());
        while inner.ended.len() > KEEP_ENDED {
            if let Some(old) = inner.ended.pop_front() {
                inner.live.remove(&old);
            }
        }
        self.wake.notify_all();
    }

    fn event(&self, key: &Key, text: &str) {
        if let Some(live) = self.lock().live.get_mut(key) {
            live.push(text);
        }
        self.wake.notify_all();
    }

    fn record_of(&self, inner: &Inner, project: &str, run: &str) -> Result<RunRecord, Error> {
        if let Some(live) = inner.live.get(&(project.to_owned(), run.to_owned())) {
            return Ok(live.record.clone());
        }
        let unknown = || Error::new(404, "run.unknown", format!("no run {run} in {project}"));
        if !valid_run(run) || !inner.projects.contains_key(project) {
            return Err(unknown());
        }
        let path = self.run_dir(project, run).join(RECORD_FILE);
        RunRecord::from_message(&read_message(&path)?.ok_or_else(unknown)?)
    }

    /// The acknowledgement of an earlier command with the same request ID, if any.
    fn replay(
        &self,
        inner: &Inner,
        request: &str,
        command: Command,
        project: &str,
        detail: &str,
    ) -> Result<Option<Ack>, Error> {
        let Some(m) = read_message(&self.ack_path(request))? else {
            return Ok(None);
        };
        let mut ack = Ack::from_message(&m)?;
        let same_detail = match command {
            Command::Submit => ack.input == detail,
            Command::Cancel => ack.run == detail,
        };
        if ack.command != command || ack.project != project || !same_detail {
            return Err(Error::new(
                409,
                "request.reused",
                format!("request {request} belongs to another command"),
            ));
        }
        ack.state = self.record_of(inner, &ack.project, &ack.run)?.state;
        ack.outcome = Outcome::Replay;
        Ok(Some(ack))
    }

    /// Copy the input into a fixed revision. The same bytes give the same revision.
    fn fix_revision(&self, project: &str, src: &Path) -> Result<String, Error> {
        let dir = self.project_dir(project).join("revisions");
        let err = |e| Error::io("revision", e);
        fs::create_dir_all(&dir).map_err(err)?;
        let tmp = dir.join(".incoming");
        let hash = copy_hashed(src, &tmp)?;
        let mut k = 0;
        loop {
            let id = match k {
                0 => format!("rev-{hash:016x}"),
                _ => format!("rev-{hash:016x}-{k}"),
            };
            let file = dir.join(&id).join(INPUT_FILE);
            if file.is_file() {
                if same_bytes(&tmp, &file)? {
                    fs::remove_file(&tmp).map_err(err)?;
                    return Ok(id);
                }
                k += 1;
                continue;
            }
            fs::create_dir_all(dir.join(&id)).map_err(err)?;
            fs::rename(&tmp, &file).map_err(err)?;
            let mut perms = fs::metadata(&file).map_err(err)?.permissions();
            perms.set_readonly(true);
            fs::set_permissions(&file, perms).map_err(err)?;
            return Ok(id);
        }
    }

    /// Run one queued run to its end.
    fn run(&self, key: &Key) {
        let (input, out, cancel, name) = {
            let mut inner = self.lock();
            let Some(live) = inner.live.get_mut(key) else {
                return;
            };
            if live.record.state != RunState::Queued {
                return;
            }
            self.change(live, RunState::Running, "");
            let input = self
                .project_dir(&key.0)
                .join("revisions")
                .join(&live.record.revision)
                .join(INPUT_FILE);
            (
                input,
                self.run_dir(&key.0, &key.1).join(OUT_DIR),
                live.cancel.clone(),
                live.record.solver.clone(),
            )
        };
        self.wake.notify_all();
        let solver = match name.as_str() {
            "" => Some(self.solver.clone()),
            name => find_solver(&self.solver_dirs, name).map(|exe| SubprocessSolver {
                exe,
                ..self.solver.clone()
            }),
        };
        let result = match solver {
            Some(solver) => solver.run_with(&input, &out, &cancel, &mut |e| {
                self.event(key, &solver_text(e));
            }),
            None => Err(ApiError::new("run.solver.missing", missing_solver(&name))),
        };
        let mut inner = self.lock();
        let Some(state) = inner.live.get(key).map(|l| l.record.state) else {
            return;
        };
        let (next, message) = match result {
            Ok(_) => (RunState::Succeeded, String::new()),
            // Only a cancel command and a daemon stop cancel a run.
            Err(e) if e.is_cancelled() && state == RunState::Cancelling => {
                (RunState::Cancelled, String::new())
            }
            Err(e) if e.is_cancelled() => (RunState::Interrupted, STOPPED.to_owned()),
            Err(e) => (RunState::Failed, e.message),
        };
        self.finish(&mut inner, key, next, &message);
    }
}

fn work(shared: Arc<Shared>) {
    loop {
        let key = {
            let mut inner = shared.lock();
            loop {
                if inner.stopping {
                    return;
                }
                if let Some(key) = inner.queue.pop_front() {
                    break key;
                }
                inner = shared
                    .wake
                    .wait(inner)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        shared.run(&key);
    }
}

fn check_request(request: &str) -> Result<(), Error> {
    if valid_request(request) {
        Ok(())
    } else {
        Err(Error::bad(
            "request.invalid",
            format!("bad request ID {request:?}"),
        ))
    }
}

fn project_root<'a>(inner: &'a Inner, project: &str) -> Result<&'a PathBuf, Error> {
    inner
        .projects
        .get(project)
        .ok_or_else(|| Error::new(404, "project.unknown", format!("no project {project}")))
}

fn stopping(inner: &Inner) -> Result<(), Error> {
    if inner.stopping {
        return Err(Error::new(503, "daemon.stopping", "the daemon is stopping"));
    }
    Ok(())
}

/// The input file of a submit. It must be a file inside the project root.
fn resolve_input(root: &Path, input: &str) -> Result<PathBuf, Error> {
    let path = fs::canonicalize(root.join(input))
        .map_err(|e| Error::bad("input.invalid", format!("{input}: {e}")))?;
    if !path.starts_with(root) {
        return Err(Error::bad(
            "input.outside",
            format!("{input} is outside the project root"),
        ));
    }
    if !path.is_file() {
        return Err(Error::bad(
            "input.invalid",
            format!("{input} is not a file"),
        ));
    }
    Ok(path)
}

impl Service {
    /// Open the store, end the runs of an earlier daemon and start the worker. Only one
    /// service at a time can use a state folder.
    pub fn open(config: Config) -> Result<Service, Error> {
        let err = |e| Error::io("state folder", e);
        fs::create_dir_all(config.state_dir.join("acks")).map_err(err)?;
        fs::create_dir_all(config.state_dir.join("projects")).map_err(err)?;
        let dir = fs::canonicalize(&config.state_dir).map_err(err)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join(LOCK_FILE))
            .map_err(err)?;
        // A child that another thread spawns gets a copy of each open file until its exec
        // closes the copy, so the lock of a stopped service in this process can stay for
        // some milliseconds. Try again for a short time before the refusal.
        let end = Instant::now() + Duration::from_secs(1);
        let mut locked = lock.try_lock();
        while matches!(locked, Err(TryLockError::WouldBlock)) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
            locked = lock.try_lock();
        }
        match locked {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(Error::new(
                    409,
                    "daemon.running",
                    "another daemon uses this state folder",
                ))
            }
            Err(TryLockError::Error(e)) => return Err(err(e)),
        }
        let shared = Arc::new(Shared {
            dir,
            instance: random_hex(8),
            solver: config.solver,
            solver_dirs: config.solver_dirs,
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
        });
        shared.recover()?;
        let worker = {
            let shared = shared.clone();
            std::thread::spawn(move || work(shared))
        };
        Ok(Service {
            shared,
            worker: Mutex::new(Some(worker)),
            lock: Mutex::new(Some(lock)),
        })
    }

    /// A random ID of this daemon instance.
    pub fn instance(&self) -> &str {
        &self.shared.instance
    }

    /// The ID of the project at `root`. The same root always gets the same ID.
    pub fn register(&self, root: &Path) -> Result<(String, PathBuf), Error> {
        let root = fs::canonicalize(root)
            .map_err(|e| Error::bad("root.invalid", format!("{}: {e}", root.display())))?;
        let text = root
            .to_str()
            .filter(|_| root.is_dir())
            .ok_or_else(|| Error::bad("root.invalid", "the root must be a UTF-8 folder path"))?
            .to_owned();
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        if let Some((id, _)) = inner.projects.iter().find(|(_, r)| **r == root) {
            return Ok((id.clone(), root));
        }
        let id = format!("p-{}", random_hex(8));
        let dir = sh.project_dir(&id);
        fs::create_dir_all(dir.join("runs")).map_err(|e| Error::io("project", e))?;
        write_atomic(&dir.join(ROOT_FILE), &format!("{text}\n"))?;
        inner.projects.insert(id.clone(), root.clone());
        inner.next_run.insert(id.clone(), 1);
        Ok((id, root))
    }

    /// Fix the input as a revision and queue a run of it.
    ///
    /// A retry with the same request ID gets the same run. A submit of a revision that
    /// already has an active run in the project with the same solver gets that run, with the
    /// outcome `existing`. `solver` is a name of [`SOLVERS`], or empty for `fh-solve`.
    pub fn submit(
        &self,
        project: &str,
        request: &str,
        input: &str,
        solver: &str,
    ) -> Result<Ack, Error> {
        check_request(request)?;
        let solver = match solver {
            "" => "",
            s if s == SOLVERS[0] => "",
            s if SOLVERS.contains(&s) => s,
            s => {
                return Err(Error::bad(
                    "solver.unknown",
                    format!("no solver {s}. The solvers: {}", SOLVERS.join(", ")),
                ))
            }
        };
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        if let Some(ack) = sh.replay(&inner, request, Command::Submit, project, input)? {
            return Ok(ack);
        }
        let path = resolve_input(project_root(&inner, project)?, input)?;
        let revision = sh.fix_revision(project, &path)?;
        let active = inner.live.values().find(|l| {
            l.record.project == project
                && l.record.revision == revision
                && l.record.solver == solver
                && !l.record.state.is_final()
        });
        let (run, state, outcome) = match active {
            Some(l) => (l.record.run.clone(), l.record.state, Outcome::Existing),
            None => {
                if inner.queue.len() >= MAX_QUEUED {
                    return Err(Error::new(
                        503,
                        "queue.full",
                        format!("{MAX_QUEUED} runs wait already"),
                    ));
                }
                let number = inner.next_run.get(project).copied().unwrap_or(1);
                let run = run_id(number);
                let dir = sh.run_dir(project, &run);
                fs::create_dir_all(&dir).map_err(|e| Error::io("run folder", e))?;
                let record = RunRecord {
                    project: project.to_owned(),
                    run: run.clone(),
                    revision,
                    input: input.to_owned(),
                    request: request.to_owned(),
                    solver: solver.to_owned(),
                    state: RunState::Queued,
                    created_ms: now_ms(),
                    started_ms: 0,
                    finished_ms: 0,
                    message: String::new(),
                };
                sh.save(&record)?;
                inner.next_run.insert(project.to_owned(), number + 1);
                let log = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join(EVENTS_FILE))
                    .ok();
                let mut live = Live {
                    record,
                    events: EventQueue::new(),
                    log,
                    log_bytes: 0,
                    log_full: false,
                    cancel: CancelToken::new(),
                };
                live.push("state queued");
                let key = (project.to_owned(), run.clone());
                inner.live.insert(key.clone(), live);
                inner.queue.push_back(key);
                (run, RunState::Queued, Outcome::Created)
            }
        };
        let ack = Ack {
            request: request.to_owned(),
            command: Command::Submit,
            project: project.to_owned(),
            run,
            input: input.to_owned(),
            state,
            outcome,
        };
        write_atomic(&sh.ack_path(request), &ack.to_message().to_text())?;
        sh.wake.notify_all();
        Ok(ack)
    }

    /// Fix the input as a revision with no run, so that the page can show the model before
    /// the solve. A submit of the same bytes later solves this revision.
    pub fn preview(&self, project: &str, input: &str) -> Result<String, Error> {
        let sh = &self.shared;
        let inner = sh.lock();
        stopping(&inner)?;
        let path = resolve_input(project_root(&inner, project)?, input)?;
        sh.fix_revision(project, &path)
    }

    /// The bytes of the input of a revision, for the page.
    pub fn revision_file(&self, project: &str, revision: &str) -> Result<Vec<u8>, Error> {
        if !valid_revision(revision) {
            return Err(Error::bad(
                "revision.invalid",
                format!("bad revision ID {revision:?}"),
            ));
        }
        let path = {
            let inner = self.shared.lock();
            project_root(&inner, project)?;
            let dir = self.shared.project_dir(project).join("revisions");
            dir.join(revision).join(INPUT_FILE)
        };
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => Error::new(
                404,
                "revision.unknown",
                format!("project {project} has no revision {revision}"),
            ),
            _ => Error::io("revision", e),
        })
    }

    /// Stop a run. A queued run ends at once. A running run gets `cancelling`, and its
    /// solver gets the protocol v2 `cancel`. A retry with the same request ID changes nothing.
    pub fn cancel(&self, project: &str, run: &str, request: &str) -> Result<Ack, Error> {
        check_request(request)?;
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        if let Some(ack) = sh.replay(&inner, request, Command::Cancel, project, run)? {
            return Ok(ack);
        }
        project_root(&inner, project)?;
        let key = (project.to_owned(), run.to_owned());
        let state = sh.record_of(&inner, project, run)?.state;
        let (state, outcome) = match state {
            RunState::Queued => {
                inner.queue.retain(|k| *k != key);
                sh.finish(
                    &mut inner,
                    &key,
                    RunState::Cancelled,
                    "cancelled before the start",
                );
                (RunState::Cancelled, Outcome::CancelRequested)
            }
            RunState::Running => {
                if let Some(live) = inner.live.get_mut(&key) {
                    sh.change(live, RunState::Cancelling, "");
                    live.cancel.cancel();
                }
                (RunState::Cancelling, Outcome::CancelRequested)
            }
            RunState::Cancelling => (state, Outcome::CancelRequested),
            _ => (state, Outcome::AlreadyFinal),
        };
        let ack = Ack {
            request: request.to_owned(),
            command: Command::Cancel,
            project: project.to_owned(),
            run: run.to_owned(),
            input: String::new(),
            state,
            outcome,
        };
        write_atomic(&sh.ack_path(request), &ack.to_message().to_text())?;
        sh.wake.notify_all();
        Ok(ack)
    }

    pub fn record(&self, project: &str, run: &str) -> Result<RunRecord, Error> {
        let inner = self.shared.lock();
        project_root(&inner, project)?;
        self.shared.record_of(&inner, project, run)
    }

    /// The records of all runs of a project, in run order.
    pub fn runs(&self, project: &str) -> Result<Vec<RunRecord>, Error> {
        let inner = self.shared.lock();
        project_root(&inner, project)?;
        let dir = self.shared.project_dir(project).join("runs");
        let mut names: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| Error::io("runs", e))?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_run(n))
            .collect();
        names.sort();
        names
            .iter()
            .map(|run| self.shared.record_of(&inner, project, run))
            .collect()
    }

    /// The registered projects and their root folders, in project ID order.
    pub fn projects(&self) -> Vec<(String, PathBuf)> {
        let inner = self.shared.lock();
        inner
            .projects
            .iter()
            .map(|(p, root)| (p.clone(), root.clone()))
            .collect()
    }

    /// The bytes of one file in the protocol v2 run folder of a run, for the viewer. `name`
    /// is a plain file name: letters, digits, `.`, `_` and `-`, with no `.` first.
    pub fn result_file(&self, project: &str, run: &str, name: &str) -> Result<Vec<u8>, Error> {
        let plain = (1..=128).contains(&name.len())
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !plain {
            return Err(Error::bad(
                "file.invalid",
                format!("{name:?} is not a plain file name"),
            ));
        }
        let path = {
            let inner = self.shared.lock();
            project_root(&inner, project)?;
            self.shared.record_of(&inner, project, run)?;
            self.shared.run_dir(project, run).join(OUT_DIR).join(name)
        };
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => {
                Error::new(404, "file.missing", format!("run {run} has no file {name}"))
            }
            _ => Error::io(name, e),
        })
    }

    /// The events of a run after `since`. With a `wait`, the call waits up to `MAX_WAIT` for
    /// a new event, unless the run has ended.
    pub fn events(
        &self,
        project: &str,
        run: &str,
        since: u64,
        wait: Duration,
    ) -> Result<Events, Error> {
        let sh = &self.shared;
        let deadline = Instant::now() + wait.min(MAX_WAIT);
        let mut inner = sh.lock();
        project_root(&inner, project)?;
        loop {
            let key = (project.to_owned(), run.to_owned());
            let Some(live) = inner.live.get(&key) else {
                let record = sh.record_of(&inner, project, run)?;
                let log = sh.run_dir(project, run).join(EVENTS_FILE);
                return Ok(EventQueue::load(&log)?.since(since, record.state));
            };
            let left = deadline.saturating_duration_since(Instant::now());
            if live.events.next - 1 > since
                || live.record.state.is_final()
                || left.is_zero()
                || inner.stopping
            {
                return Ok(live.events.since(since, live.record.state));
            }
            inner = sh
                .wake
                .wait_timeout(inner, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Stop the service: end the queued runs, cancel the running run, wait for the worker
    /// and free the state folder. The runs end as `interrupted`.
    pub fn stop(&self) {
        let sh = &self.shared;
        {
            let mut inner = sh.lock();
            if !inner.stopping {
                inner.stopping = true;
                let queued: Vec<Key> = inner.queue.drain(..).collect();
                for key in &queued {
                    sh.finish(&mut inner, key, RunState::Interrupted, STOPPED);
                }
                for live in inner.live.values() {
                    if !live.record.state.is_final() {
                        live.cancel.cancel();
                    }
                }
            }
        }
        sh.wake.notify_all();
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        self.lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    /// Load the projects and end each run that an earlier daemon left active. A queued or
    /// running run becomes `interrupted`, and a run with a pending cancel becomes
    /// `cancelled`. The daemon never starts these runs again by itself.
    fn recover(&self) -> Result<(), Error> {
        let err = |e| Error::io("recover", e);
        let mut inner = self.lock();
        for entry in fs::read_dir(self.dir.join("projects"))
            .map_err(err)?
            .flatten()
        {
            let Ok(project) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(root) = fs::read_to_string(entry.path().join(ROOT_FILE)) else {
                continue;
            };
            if !valid_project(&project) {
                continue;
            }
            let mut next = 1;
            let runs = entry.path().join("runs");
            for run in fs::read_dir(&runs).map_err(err)?.flatten() {
                let Ok(name) = run.file_name().into_string() else {
                    continue;
                };
                let Some(number) = name
                    .strip_prefix("run-")
                    .and_then(|n| n.parse::<u64>().ok())
                else {
                    continue;
                };
                next = next.max(number + 1);
                let Some(m) = read_message(&run.path().join(RECORD_FILE))? else {
                    continue;
                };
                let mut record = RunRecord::from_message(&m)?;
                if record.state.is_final() {
                    continue;
                }
                (record.state, record.message) = if record.state == RunState::Cancelling {
                    (
                        RunState::Cancelled,
                        "the daemon stopped during the cancel".to_owned(),
                    )
                } else {
                    (RunState::Interrupted, STOPPED.to_owned())
                };
                record.finished_ms = now_ms();
                self.save(&record)?;
                // The daemon can stop after it wrote the record and before it wrote the
                // acknowledgement. Write the acknowledgement now, so a retry finds this run.
                let ack = self.ack_path(&record.request);
                if !ack.exists() {
                    let created = Ack {
                        request: record.request.clone(),
                        command: Command::Submit,
                        project: record.project.clone(),
                        run: record.run.clone(),
                        input: record.input.clone(),
                        state: record.state,
                        outcome: Outcome::Created,
                    };
                    write_atomic(&ack, &created.to_message().to_text())?;
                }
            }
            inner
                .projects
                .insert(project.clone(), PathBuf::from(root.trim_end_matches('\n')));
            inner.next_run.insert(project, next);
        }
        self.prune_acks()
    }

    /// Keep the newest `MAX_ACKS` acknowledgements.
    fn prune_acks(&self) -> Result<(), Error> {
        let dir = self.dir.join("acks");
        let mut acks: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&dir)
            .map_err(|e| Error::io("acks", e))?
            .flatten()
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        if acks.len() > MAX_ACKS {
            acks.sort();
            for (_, path) in &acks[..acks.len() - MAX_ACKS] {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_event_queue_is_bounded_and_counts_dropped_events() {
        let mut q = EventQueue::new();
        for i in 0..EVENT_QUEUE + 10 {
            q.push(format!("log {i}"));
        }
        let all = q.since(0, RunState::Running);
        assert_eq!(all.dropped, 10);
        assert_eq!(all.last, (EVENT_QUEUE + 10) as u64);
        assert_eq!(all.items.len(), EVENT_BATCH);
        assert_eq!(all.items[0].seq, 11);
        let tail = q.since(all.last - 1, RunState::Running);
        assert_eq!((tail.dropped, tail.items.len()), (0, 1));
    }

    #[test]
    fn a_long_event_is_cut_at_a_char_boundary() {
        let text = "é".repeat(EVENT_TEXT);
        let cut = clip(&text, EVENT_TEXT);
        assert_eq!(cut.len(), EVENT_TEXT);
        assert!(clip("ab", 1) == "a");
    }
}
