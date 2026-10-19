//! The run service: the store on disk, the run queue and the event queues. The HTTP
//! transport and an in-process caller use the same commands.
//!
//! # Store
//!
//! ```text
//! <state>/daemon.lock                 exclusive lock of the one daemon
//! <state>/daemon.info                 port and token for clients (mode 0600)
//! <state>/runs/<run>/in/input.inp     the read-only copy of the input that the run solves, with
//!                                     the copies of its `*INCLUDE` files
//! <state>/runs/<run>/record           the run record
//! <state>/runs/<run>/events.log       bounded event log
//! <state>/runs/<run>/out/             run folder of ccx
//! <state>/previews/<n>/input.inp      the copy of the input of a preview, with its `*INCLUDE` files
//! ```
//!
//! Each record is written to a temporary file and then renamed, so a reader never sees a
//! partial file.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fh_solver_ccx::{input_text, stage_input, CancelToken, Ccx};

use crate::contract::{
    now_ms, parse_event, run_id, valid_run, Error, Event, Events, Message, RunRecord, RunState,
};

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
/// The number of ended runs whose events stay in memory. The others come from their log.
const KEEP_ENDED: usize = 32;

pub(crate) const LOCK_FILE: &str = "daemon.lock";
const RUNS_DIR: &str = "runs";
const PREVIEWS_DIR: &str = "previews";
const RECORD_FILE: &str = "record";
const EVENTS_FILE: &str = "events.log";
const INPUT_FILE: &str = "input.inp";
const IN_DIR: &str = "in";
const OUT_DIR: &str = "out";
const STOPPED: &str = "the daemon stopped before the run ended";

/// The settings of a run service.
pub struct Config {
    /// The folder of the store. One daemon at a time uses it.
    pub state_dir: PathBuf,
    /// The `ccx` program, with its limits.
    pub solver: Ccx,
}

/// The limits of one run. A field that is `None` keeps the limit of the daemon.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Limits {
    pub timeout: Option<Duration>,
    /// The memory limit in bytes.
    pub memory: Option<u64>,
}

/// The run service. It owns every run. Clients only send commands and read records.
pub struct Service {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
    lock: Mutex<Option<File>>,
}

struct Shared {
    dir: PathBuf,
    solver: Ccx,
    inner: Mutex<Inner>,
    /// Wakes the worker for a new run, and the readers for a new event.
    wake: Condvar,
}

#[derive(Default)]
struct Inner {
    next_run: u64,
    next_preview: u64,
    /// The runs of this daemon instance, with their event queues, by run ID.
    live: BTreeMap<String, Live>,
    queue: VecDeque<String>,
    ended: VecDeque<String>,
    stopping: bool,
}

struct Live {
    record: RunRecord,
    events: EventQueue,
    log: Option<File>,
    log_bytes: u64,
    log_full: bool,
    cancel: CancelToken,
    /// The solver with the limits of this run.
    solver: Ccx,
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

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run_dir(&self, run: &str) -> PathBuf {
        self.dir.join(RUNS_DIR).join(run)
    }

    fn save(&self, record: &RunRecord) -> Result<(), Error> {
        let path = self.run_dir(&record.run).join(RECORD_FILE);
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
        // The state event has the reason, so that a client that reads only the events (or
        // the event log) sees why a run failed.
        if message.is_empty() {
            live.push(&format!("state {state}"));
        } else {
            live.record.message = message.to_owned();
            live.push(&format!("state {state}: {message}"));
        }
        // A record that cannot be written keeps its earlier state on disk. A later daemon
        // then shows the run as interrupted, so the record never shows a false success.
        let _ = self.save(&live.record);
    }

    fn finish(&self, inner: &mut Inner, run: &str, state: RunState, message: &str) {
        if let Some(live) = inner.live.get_mut(run) {
            self.change(live, state, message);
            live.log = None;
        }
        inner.ended.push_back(run.to_owned());
        while inner.ended.len() > KEEP_ENDED {
            if let Some(old) = inner.ended.pop_front() {
                inner.live.remove(&old);
            }
        }
        self.wake.notify_all();
    }

    fn event(&self, run: &str, text: &str) {
        if let Some(live) = self.lock().live.get_mut(run) {
            live.push(text);
        }
        self.wake.notify_all();
    }

    /// The record of a run. A run of an earlier daemon that did not end shows as
    /// `interrupted`, because no daemon runs it now.
    fn record_of(&self, inner: &Inner, run: &str) -> Result<RunRecord, Error> {
        if let Some(live) = inner.live.get(run) {
            return Ok(live.record.clone());
        }
        let unknown = || Error::new(404, "run.unknown", format!("no run {run}"));
        if !valid_run(run) {
            return Err(unknown());
        }
        let path = self.run_dir(run).join(RECORD_FILE);
        let mut record = RunRecord::from_message(&read_message(&path)?.ok_or_else(unknown)?)?;
        if !record.state.is_final() {
            (record.state, record.message) = (RunState::Interrupted, STOPPED.to_owned());
        }
        Ok(record)
    }

    /// The bytes of the file `name` in the folder of a run.
    fn run_file(&self, run: &str, name: &Path) -> Result<Vec<u8>, Error> {
        let path = {
            let inner = self.lock();
            self.record_of(&inner, run)?;
            self.run_dir(run).join(name)
        };
        fs::read(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => Error::new(
                404,
                "file.missing",
                format!("run {run} has no file {}", name.display()),
            ),
            _ => Error::io(&name.display().to_string(), e),
        })
    }

    /// Run one queued run to its end.
    fn run(&self, run: &str) {
        let (cancel, solver) = {
            let mut inner = self.lock();
            let Some(live) = inner.live.get_mut(run) else {
                return;
            };
            if live.record.state != RunState::Queued {
                return;
            }
            self.change(live, RunState::Running, "");
            (live.cancel.clone(), live.solver.clone())
        };
        self.wake.notify_all();
        let dir = self.run_dir(run);
        let result = solver.run_with(
            &dir.join(IN_DIR).join(INPUT_FILE),
            &dir.join(OUT_DIR),
            &cancel,
            &mut |p| {
                self.event(
                    run,
                    &format!("progress step={} increment={}", p.step, p.increment),
                )
            },
        );
        let mut inner = self.lock();
        let Some(state) = inner.live.get(run).map(|l| l.record.state) else {
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
        self.finish(&mut inner, run, next, &message);
    }
}

fn work(shared: Arc<Shared>) {
    loop {
        let run = {
            let mut inner = shared.lock();
            loop {
                if inner.stopping {
                    return;
                }
                if let Some(run) = inner.queue.pop_front() {
                    break run;
                }
                inner = shared
                    .wake
                    .wait(inner)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        shared.run(&run);
    }
}

fn stopping(inner: &Inner) -> Result<(), Error> {
    if inner.stopping {
        return Err(Error::new(503, "daemon.stopping", "the daemon is stopping"));
    }
    Ok(())
}

/// Copy the input file and its include files into `dir`. A missing include file is a bad input.
fn stage(input: &Path, dir: &Path) -> Result<(), Error> {
    stage_input(input, dir, INPUT_FILE).map_err(|e| Error::bad("input.invalid", e.message))
}

/// The text of a copy of an input, with its include files.
fn flat_text(path: &Path) -> Result<Vec<u8>, Error> {
    input_text(path)
        .map(String::into_bytes)
        .map_err(|e| Error::new(500, "input.unreadable", e.message))
}

/// The input file of a submit or a preview: the absolute path of a file.
fn resolve_input(input: &str) -> Result<&Path, Error> {
    let path = Path::new(input);
    if !path.is_absolute() || !path.is_file() {
        return Err(Error::bad(
            "input.invalid",
            format!("{input} is not the absolute path of a file"),
        ));
    }
    Ok(path)
}

impl Service {
    /// Open the store and start the worker. Only one service at a time can use a state
    /// folder.
    pub fn open(config: Config) -> Result<Service, Error> {
        let err = |e| Error::io("state folder", e);
        fs::create_dir_all(config.state_dir.join(RUNS_DIR)).map_err(err)?;
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
        // Only the chat of this daemon shows a preview, so the previews of an earlier
        // daemon go.
        let previews = dir.join(PREVIEWS_DIR);
        let _ = fs::remove_dir_all(&previews);
        fs::create_dir_all(&previews).map_err(err)?;
        let last = fs::read_dir(dir.join(RUNS_DIR))
            .map_err(err)?
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_prefix("run-")?.parse().ok())
            .max()
            .unwrap_or(0);
        let shared = Arc::new(Shared {
            dir,
            solver: config.solver,
            inner: Mutex::new(Inner {
                next_run: last + 1,
                next_preview: 1,
                ..Inner::default()
            }),
            wake: Condvar::new(),
        });
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

    /// Copy the input file and its `*INCLUDE` files, and queue a run of the copy. `input` is
    /// the absolute path of the file. A later change of a file does not change the run.
    pub fn submit(&self, input: &str, limits: Limits) -> Result<RunRecord, Error> {
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        let path = resolve_input(input)?;
        if inner.queue.len() >= MAX_QUEUED {
            return Err(Error::new(
                503,
                "queue.full",
                format!("{MAX_QUEUED} runs wait already"),
            ));
        }
        let run = run_id(inner.next_run);
        let dir = sh.run_dir(&run);
        fs::create_dir_all(&dir).map_err(|e| Error::io("run folder", e))?;
        inner.next_run += 1;
        stage(path, &dir.join(IN_DIR))?;
        let record = RunRecord {
            run: run.clone(),
            input: input.to_owned(),
            state: RunState::Queued,
            created_ms: now_ms(),
            started_ms: 0,
            finished_ms: 0,
            message: String::new(),
        };
        sh.save(&record)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(EVENTS_FILE))
            .ok();
        let mut live = Live {
            record: record.clone(),
            events: EventQueue::new(),
            log,
            log_bytes: 0,
            log_full: false,
            cancel: CancelToken::new(),
            solver: Ccx {
                timeout: limits.timeout.or(sh.solver.timeout),
                memory_limit: limits.memory.unwrap_or(sh.solver.memory_limit),
                ..sh.solver.clone()
            },
        };
        live.push("state queued");
        inner.live.insert(run.clone(), live);
        inner.queue.push_back(run);
        sh.wake.notify_all();
        Ok(record)
    }

    /// Copy the input file and its `*INCLUDE` files with no run, so that the page can show the
    /// model before the solve. The answer is the preview ID.
    pub fn preview(&self, input: &str) -> Result<String, Error> {
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        let path = resolve_input(input)?;
        let id = inner.next_preview.to_string();
        stage(path, &sh.dir.join(PREVIEWS_DIR).join(&id))?;
        inner.next_preview += 1;
        Ok(id)
    }

    /// The text of the input of a preview, for the page. It holds the text of the include files.
    pub fn preview_file(&self, id: &str) -> Result<Vec<u8>, Error> {
        let unknown = || Error::new(404, "preview.unknown", format!("no preview {id:?}"));
        let n: u64 = id.parse().map_err(|_| unknown())?;
        let path = self
            .shared
            .dir
            .join(PREVIEWS_DIR)
            .join(n.to_string())
            .join(INPUT_FILE);
        if !path.is_file() {
            return Err(unknown());
        }
        flat_text(&path)
    }

    /// The text of the copy of the input that a run solves, for the page. It holds the text of
    /// the include files.
    pub fn input_file(&self, run: &str) -> Result<Vec<u8>, Error> {
        let path = {
            let inner = self.shared.lock();
            self.shared.record_of(&inner, run)?;
            self.shared.run_dir(run).join(IN_DIR).join(INPUT_FILE)
        };
        flat_text(&path)
    }

    /// Stop a run. A queued run ends at once. A running run gets `cancelling`, and the
    /// run kills `ccx`. A cancel of a final run changes nothing. The
    /// answer is the record after the cancel.
    pub fn cancel(&self, run: &str) -> Result<RunRecord, Error> {
        let sh = &self.shared;
        let mut inner = sh.lock();
        stopping(&inner)?;
        match sh.record_of(&inner, run)?.state {
            RunState::Queued => {
                inner.queue.retain(|r| r != run);
                let why = "cancelled before the start";
                sh.finish(&mut inner, run, RunState::Cancelled, why);
            }
            RunState::Running => {
                if let Some(live) = inner.live.get_mut(run) {
                    sh.change(live, RunState::Cancelling, "");
                    live.cancel.cancel();
                }
            }
            _ => {}
        }
        sh.wake.notify_all();
        sh.record_of(&inner, run)
    }

    pub fn record(&self, run: &str) -> Result<RunRecord, Error> {
        self.shared.record_of(&self.shared.lock(), run)
    }

    /// The records of all runs, in run order. A run folder with no record is not a run.
    pub fn runs(&self) -> Result<Vec<RunRecord>, Error> {
        let inner = self.shared.lock();
        let dir = self.shared.dir.join(RUNS_DIR);
        let mut names: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| Error::io("runs", e))?
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_run(n))
            .collect();
        names.sort();
        let records = names.iter().map(|run| self.shared.record_of(&inner, run));
        Ok(records.filter_map(Result::ok).collect())
    }

    /// The runs of this daemon that are not final, in run order, each with the text of its
    /// last `progress` event (or an empty text).
    pub fn active(&self) -> Vec<(RunRecord, String)> {
        let inner = self.shared.lock();
        let live = inner.live.values().filter(|l| !l.record.state.is_final());
        live.map(|l| {
            let items = l.events.items.iter().rev();
            let progress = items.map(|e| &e.text).find(|t| t.starts_with("progress "));
            (l.record.clone(), progress.cloned().unwrap_or_default())
        })
        .collect()
    }

    /// The bytes of one file in the run folder of a run, for the viewer. `name`
    /// is a plain file name: letters, digits, `.`, `_` and `-`, with no `.` first.
    pub fn result_file(&self, run: &str, name: &str) -> Result<Vec<u8>, Error> {
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
        self.shared.run_file(run, &Path::new(OUT_DIR).join(name))
    }

    /// The events of a run after `since`. With a `wait`, the call waits up to `MAX_WAIT` for
    /// a new event, unless the run has ended.
    pub fn events(&self, run: &str, since: u64, wait: Duration) -> Result<Events, Error> {
        let sh = &self.shared;
        let deadline = Instant::now() + wait.min(MAX_WAIT);
        let mut inner = sh.lock();
        loop {
            let Some(live) = inner.live.get(run) else {
                let record = sh.record_of(&inner, run)?;
                let log = sh.run_dir(run).join(EVENTS_FILE);
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
                let queued: Vec<String> = inner.queue.drain(..).collect();
                for run in &queued {
                    sh.finish(&mut inner, run, RunState::Interrupted, STOPPED);
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
