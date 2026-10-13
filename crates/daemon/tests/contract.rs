//! The D1 exit checks of the run contract, with a shell script as a protocol v2 solver.
//!
//! Browser close, CLI reconnect, a duplicate submission, cancel and a daemon restart each
//! keep one correct run record. A foreign browser `Origin` fails. An authorized CLI passes.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fh_daemon::{Client, Config, Daemon, Outcome, RunRecord, RunState};
use fh_solver_api::SubprocessSolver;

/// A solver that runs until a cancel or the end of stdin when the input has the word
/// `slow`. Other inputs wait for the file `gate` and then write good results.
const SOLVER: &str = r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "start $1" >> DIR/started.log
if grep -q slow "$1"; then
  echo $$ > DIR/solver.pid
  echo "progress step=1/1 phase=solve fraction=0.1"
  while read line; do
    if [ "$line" = cancel ]; then echo cancelled > DIR/solver.end; exit 130; fi
  done
  echo eof > DIR/solver.end
  exit 130
fi
if grep -q chatty "$1"; then
  i=0
  while [ $i -lt 2000 ]; do printf 'line %01000d\n' $i; i=$((i+1)); done
fi
while [ ! -e DIR/gate ]; do sleep 0.02; done
echo "progress step=1/1 phase=solve fraction=1"
echo '<VTKFile type="Collection"><Collection><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>' > results.pvd
echo "<VTKFile/>" > step-1.vtu
printf 'schema: fh.run\nversion: 1.1.0\nstatus: ok\n' > summary.toon
"#;

struct Lab {
    dir: PathBuf,
    state: PathBuf,
    root: PathBuf,
    solver: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fh-daemon-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let root = dir.join("project");
        fs::create_dir_all(&root).unwrap();
        let solver = dir.join("solver.sh");
        fs::write(&solver, SOLVER.replace("DIR", dir.to_str().unwrap())).unwrap();
        fs::set_permissions(&solver, fs::Permissions::from_mode(0o755)).unwrap();
        Lab {
            state: dir.join("state"),
            dir,
            root,
            solver,
        }
    }

    fn config(&self) -> Config {
        Config {
            state_dir: self.state.clone(),
            solver: SubprocessSolver::new(&self.solver),
        }
    }

    fn start(&self) -> Daemon {
        Daemon::start(self.config(), 0).unwrap()
    }

    fn input(&self, name: &str, text: &str) -> String {
        fs::write(self.root.join(name), text).unwrap();
        name.to_owned()
    }

    fn open_gate(&self) {
        fs::write(self.dir.join("gate"), "").unwrap();
    }

    fn starts(&self) -> usize {
        fs::read_to_string(self.dir.join("started.log")).map_or(0, |t| t.lines().count())
    }

    fn file(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.dir.join(name)).ok()
    }
}

fn wait_for(c: &Client, project: &str, run: &str, want: fn(RunState) -> bool) -> RunRecord {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut since = 0;
    loop {
        let events = c
            .events(project, run, since, Duration::from_millis(500))
            .unwrap();
        since = events.last;
        if want(events.state) {
            return c.record(project, run).unwrap();
        }
        assert!(
            Instant::now() < deadline,
            "run {run} stays {}",
            events.state
        );
    }
}

fn wait_file(lab: &Lab, name: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(text) = lab.file(name).filter(|t| t.ends_with('\n')) {
            return text;
        }
        assert!(Instant::now() < deadline, "no file {name}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Send a request and close the connection before the answer.
fn send_and_close(port: u16, token: &str, origin: Option<&str>, head: &str, body: &str) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    write!(
        s,
        "{head} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n{origin}\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

#[test]
fn an_authorized_cli_passes_and_a_foreign_origin_fails() {
    let lab = Lab::new("origin");
    let daemon = lab.start();
    let cli = Client::discover(&lab.state).unwrap();
    assert_eq!(cli.port(), daemon.port());
    assert!(cli.status().is_ok());
    let project = cli.register(&lab.root).unwrap();
    assert_eq!(cli.register(&lab.root).unwrap(), project);
    let input = lab.input("a.inp", "*NODE\n");

    let foreign = daemon.client().with_origin("http://evil.example");
    for result in [
        foreign.status().map(|_| ()),
        foreign.submit(&project, "evil-1", &input).map(|_| ()),
        foreign.runs(&project).map(|_| ()),
    ] {
        let e = result.unwrap_err();
        assert_eq!((e.status, e.code.as_str()), (403, "origin.refused"));
    }
    let null = daemon.client().with_origin("null").status().unwrap_err();
    assert_eq!(null.code, "origin.refused");
    let no_token = Client::new(daemon.port(), "").status().unwrap_err();
    assert_eq!(
        (no_token.status, no_token.code.as_str()),
        (401, "token.missing")
    );
    let bad_token = Client::new(daemon.port(), "0".repeat(64))
        .status()
        .unwrap_err();
    assert_eq!(bad_token.code, "token.invalid");
    assert!(
        cli.runs(&project).unwrap().is_empty(),
        "a refused submit made a run"
    );

    let own = daemon
        .client()
        .with_origin(&format!("http://127.0.0.1:{}", daemon.port()));
    let ack = own.submit(&project, "page-1", &input).unwrap();
    assert_eq!(ack.outcome, Outcome::Created);
    let ack = cli.submit(&project, "cli-1", &input).unwrap();
    assert_eq!(
        (ack.outcome, ack.run.as_str()),
        (Outcome::Existing, "run-000001")
    );
    lab.open_gate();
    let record = wait_for(&cli, &project, &ack.run, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded);
    // Only the user can read the token.
    let info = fs::metadata(lab.state.join(fh_daemon::INFO_FILE)).unwrap();
    assert_eq!(info.permissions().mode() & 0o777, 0o600);
}

#[test]
fn a_duplicate_submission_keeps_one_run() {
    let lab = Lab::new("duplicate");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let input = lab.input("a.inp", "*NODE\n");

    let first = c.submit(&project, "dup-1", &input).unwrap();
    assert_eq!(
        (first.outcome, first.state),
        (Outcome::Created, RunState::Queued)
    );
    let again = c.submit(&project, "dup-1", &input).unwrap();
    assert_eq!(
        (again.outcome, again.run.as_str()),
        (Outcome::Replay, "run-000001")
    );
    let other = c.submit(&project, "dup-2", &input).unwrap();
    assert_eq!(
        (other.outcome, other.run.as_str()),
        (Outcome::Existing, "run-000001")
    );
    let reused = c.submit(&project, "dup-1", "b.inp").unwrap_err();
    assert_eq!(
        (reused.status, reused.code.as_str()),
        (409, "request.reused")
    );

    lab.open_gate();
    wait_for(&c, &project, &first.run, RunState::is_final);
    let late = c.submit(&project, "dup-1", &input).unwrap();
    assert_eq!(
        (late.outcome, late.state),
        (Outcome::Replay, RunState::Succeeded)
    );
    assert_eq!(
        c.runs(&project).unwrap(),
        [("run-000001".to_owned(), RunState::Succeeded)]
    );
    assert_eq!(lab.starts(), 1, "the solver ran more than once");

    // A new submit of the same file after the end is a new run of the same revision.
    let next = c.submit(&project, "dup-3", &input).unwrap();
    assert_eq!(
        (next.outcome, next.run.as_str()),
        (Outcome::Created, "run-000002")
    );
    let record = wait_for(&c, &project, &next.run, RunState::is_final);
    assert_eq!(
        record.revision,
        c.record(&project, "run-000001").unwrap().revision
    );
}

#[test]
fn a_cli_that_reconnects_gets_the_same_run() {
    let lab = Lab::new("reconnect");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let input = lab.input("a.inp", "*NODE\n");

    // The connection breaks before the answer of the submit.
    send_and_close(
        daemon.port(),
        daemon.token(),
        None,
        &format!("POST /api/v1/projects/{project}/runs"),
        &format!("request=lost-1\ninput={input}\n"),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while c.runs(&project).unwrap().is_empty() {
        assert!(
            Instant::now() < deadline,
            "the daemon did not take the submit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // The CLI connects again and sends the same command.
    let ack = Client::discover(&lab.state)
        .unwrap()
        .submit(&project, "lost-1", &input)
        .unwrap();
    assert_eq!(
        (ack.outcome, ack.run.as_str()),
        (Outcome::Replay, "run-000001")
    );

    // A client that follows the events over many connections sees each event once.
    lab.open_gate();
    let mut seen = Vec::new();
    let mut since = 0;
    loop {
        let events = Client::discover(&lab.state)
            .unwrap()
            .events(&project, &ack.run, since, Duration::from_secs(5))
            .unwrap();
        assert_eq!(events.dropped, 0);
        seen.extend(events.items.iter().map(|e| e.seq));
        since = events.last;
        if events.state.is_final() && events.items.is_empty() {
            break;
        }
    }
    assert_eq!(seen, (1..=since).collect::<Vec<_>>());
    assert_eq!(c.runs(&project).unwrap().len(), 1);
    assert_eq!(
        c.record(&project, &ack.run).unwrap().state,
        RunState::Succeeded
    );
}

#[test]
fn a_closed_browser_does_not_stop_the_run() {
    let lab = Lab::new("browser");
    let daemon = lab.start();
    let page = format!("http://127.0.0.1:{}", daemon.port());
    let browser = daemon.client().with_origin(&page);
    let project = browser.register(&lab.root).unwrap();
    let input = lab.input("a.inp", "*NODE\n");
    let ack = browser.submit(&project, "page-1", &input).unwrap();
    wait_for(&browser, &project, &ack.run, |s| s == RunState::Running);

    // The browser waits for events, and then the tab closes.
    send_and_close(
        daemon.port(),
        daemon.token(),
        Some(&page),
        &format!(
            "GET /api/v1/projects/{project}/runs/{}/events?since=100&wait_ms=20000",
            ack.run
        ),
        "",
    );
    std::thread::sleep(Duration::from_millis(100));
    lab.open_gate();

    let cli = Client::discover(&lab.state).unwrap();
    let record = wait_for(&cli, &project, &ack.run, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded);
    assert_eq!(cli.runs(&project).unwrap().len(), 1);
    assert_eq!(lab.starts(), 1);
}

#[test]
fn cancel_keeps_one_record() {
    let lab = Lab::new("cancel");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let slow = lab.input("slow.inp", "** slow\n");
    let queued = lab.input("b.inp", "*NODE\n");

    let run = c.submit(&project, "run-1", &slow).unwrap().run;
    let waiting = c.submit(&project, "run-2", &queued).unwrap().run;
    wait_for(&c, &project, &run, |s| s == RunState::Running);
    assert_eq!(
        c.record(&project, &waiting).unwrap().state,
        RunState::Queued
    );

    // A queued run ends at once and never starts.
    let ack = c.cancel(&project, &waiting, "cancel-2").unwrap();
    assert_eq!(
        (ack.outcome, ack.state),
        (Outcome::CancelRequested, RunState::Cancelled)
    );

    let ack = c.cancel(&project, &run, "cancel-1").unwrap();
    assert_eq!(
        (ack.outcome, ack.state),
        (Outcome::CancelRequested, RunState::Cancelling)
    );
    let again = c.cancel(&project, &run, "cancel-1").unwrap();
    assert_eq!(again.outcome, Outcome::Replay);
    let record = wait_for(&c, &project, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Cancelled);
    assert_eq!(wait_file(&lab, "solver.end"), "cancelled\n");
    let late = c.cancel(&project, &run, "cancel-3").unwrap();
    assert_eq!(
        (late.outcome, late.state),
        (Outcome::AlreadyFinal, RunState::Cancelled)
    );

    let unknown = c.cancel(&project, "run-000009", "cancel-4").unwrap_err();
    assert_eq!(unknown.status, 404);
    assert_eq!(
        c.runs(&project).unwrap(),
        [
            ("run-000001".to_owned(), RunState::Cancelled),
            ("run-000002".to_owned(), RunState::Cancelled),
        ]
    );
    assert_eq!(lab.starts(), 1, "the cancelled queued run started");
    drop(daemon);

    // The records stay after a restart.
    let daemon = lab.start();
    let c = daemon.client();
    assert_eq!(c.record(&project, &run).unwrap(), record);
    let events = c.events(&project, &run, 0, Duration::ZERO).unwrap();
    let texts: Vec<&str> = events.items.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(texts.first(), Some(&"state queued"));
    assert_eq!(texts.last(), Some(&"state cancelled"));
}

#[test]
fn the_revision_is_fixed_at_submit() {
    let lab = Lab::new("revision");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let input = lab.input("a.inp", "*NODE\n1, 0, 0, 0\n");
    let first = c.submit(&project, "rev-1", &input).unwrap();
    // The user changes the file while the run waits.
    lab.input("a.inp", "*NODE\n1, 9, 9, 9\n");
    let second = c.submit(&project, "rev-2", &input).unwrap();
    assert_eq!(second.outcome, Outcome::Created);
    let (r1, r2) = (
        c.record(&project, &first.run).unwrap(),
        c.record(&project, &second.run).unwrap(),
    );
    assert_ne!(r1.revision, r2.revision);
    let fixed = lab
        .state
        .join("projects")
        .join(&project)
        .join("revisions")
        .join(&r1.revision)
        .join("input.inp");
    assert_eq!(fs::read_to_string(&fixed).unwrap(), "*NODE\n1, 0, 0, 0\n");
    assert!(fs::metadata(&fixed).unwrap().permissions().readonly());
    // The revision ID is the FNV-1a hash of the bytes, as `source_hash` of `fh.run`.
    let hash = b"*NODE\n1, 0, 0, 0\n"
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    assert_eq!(r1.revision, format!("rev-{hash:016x}"));

    let outside = c.submit(&project, "rev-3", "../solver.sh").unwrap_err();
    assert_eq!(outside.code, "input.outside");
    lab.open_gate();
    wait_for(&c, &project, &second.run, RunState::is_final);
}

#[test]
fn the_event_log_and_queue_are_bounded() {
    let lab = Lab::new("bounded");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let input = lab.input("a.inp", "** chatty\n");
    lab.open_gate();
    let run = c.submit(&project, "chatty-1", &input).unwrap().run;
    let record = wait_for(&c, &project, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded, "{}", record.message);
    let events = c.events(&project, &run, 0, Duration::ZERO).unwrap();
    assert!(events.dropped > 0 && events.last > fh_daemon::EVENT_QUEUE as u64);
    assert!(events
        .items
        .iter()
        .all(|e| e.text.len() <= fh_daemon::EVENT_TEXT));
    let log = lab
        .state
        .join("projects")
        .join(&project)
        .join("runs")
        .join(&run)
        .join("events.log");
    let size = fs::metadata(&log).unwrap().len();
    assert!(
        size <= fh_daemon::EVENT_LOG + 4096,
        "event log has {size} bytes"
    );
    let text = fs::read_to_string(&log).unwrap();
    assert!(text.contains(" log-full "));
    assert!(text.trim_end().ends_with("state succeeded"));
}

#[test]
fn a_second_daemon_cannot_use_the_same_state() {
    let lab = Lab::new("lock");
    let _daemon = lab.start();
    let e = Daemon::start(lab.config(), 0).err().unwrap();
    assert_eq!(e.code, "daemon.running");
}

#[test]
fn a_stopped_daemon_leaves_one_interrupted_record() {
    let lab = Lab::new("stop");
    let daemon = lab.start();
    let c = daemon.client();
    let project = c.register(&lab.root).unwrap();
    let slow = lab.input("slow.inp", "** slow\n");
    let queued = lab.input("b.inp", "*NODE\n");
    let run = c.submit(&project, "stop-1", &slow).unwrap().run;
    let waiting = c.submit(&project, "stop-2", &queued).unwrap().run;
    wait_for(&c, &project, &run, |s| s == RunState::Running);
    let old_token = daemon.token().to_owned();
    daemon.stop();
    assert_eq!(lab.file("solver.end").as_deref(), Some("cancelled\n"));

    let daemon = lab.start();
    let stale = Client::new(daemon.port(), old_token).status().unwrap_err();
    assert_eq!(stale.code, "token.invalid");
    let c = Client::discover(&lab.state).unwrap();
    assert_eq!(
        c.runs(&project).unwrap(),
        [
            (run.clone(), RunState::Interrupted),
            (waiting, RunState::Interrupted),
        ]
    );
    let ack = c.submit(&project, "stop-1", &slow).unwrap();
    assert_eq!(
        (ack.outcome, ack.run, ack.state),
        (Outcome::Replay, run, RunState::Interrupted)
    );
    assert_eq!(lab.starts(), 1);
}

/// The daemon of `a_failed_daemon_leaves_one_interrupted_record`, in a child process.
#[test]
fn daemon_child() {
    let (Ok(state), Ok(solver)) = (
        std::env::var("FH_DAEMON_CHILD_STATE"),
        std::env::var("FH_DAEMON_CHILD_SOLVER"),
    ) else {
        return;
    };
    let config = Config {
        state_dir: state.into(),
        solver: SubprocessSolver::new(solver),
    };
    let _daemon = Daemon::start(config, 0).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn a_failed_daemon_leaves_one_interrupted_record() {
    let lab = Lab::new("crash");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "daemon_child",
            "--exact",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env("FH_DAEMON_CHILD_STATE", &lab.state)
        .env("FH_DAEMON_CHILD_SOLVER", &lab.solver)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let c = loop {
        if let Some(c) = Client::discover(&lab.state)
            .ok()
            .filter(|c| c.status().is_ok())
        {
            break c;
        }
        assert!(Instant::now() < deadline, "the child daemon did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    let project = c.register(&lab.root).unwrap();
    let slow = lab.input("slow.inp", "** slow\n");
    let run = c.submit(&project, "crash-1", &slow).unwrap().run;
    wait_for(&c, &project, &run, |s| s == RunState::Running);
    let pid = wait_file(&lab, "solver.pid");

    // SIGKILL: the daemon cannot clean up.
    child.kill().unwrap();
    child.wait().unwrap();
    // The solver sees the end of its stdin and stops (protocol v2).
    assert_eq!(wait_file(&lab, "solver.end"), "eof\n");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
    {
        assert!(
            Instant::now() < deadline,
            "the solver lives after the daemon"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let daemon = lab.start();
    let c = daemon.client();
    let record = c.record(&project, &run).unwrap();
    assert_eq!(record.state, RunState::Interrupted);
    assert!(record.finished_ms >= record.started_ms && record.started_ms > 0);
    assert_eq!(
        c.runs(&project).unwrap(),
        [(run.clone(), RunState::Interrupted)]
    );
    let ack = c.submit(&project, "crash-1", &slow).unwrap();
    assert_eq!(
        (ack.outcome, ack.run.as_str()),
        (Outcome::Replay, run.as_str())
    );
    let cancel = c.cancel(&project, &run, "crash-cancel").unwrap();
    assert_eq!(cancel.outcome, Outcome::AlreadyFinal);
    // The run counter goes on. No run ID is used twice.
    let next = c.submit(&project, "crash-2", &slow).unwrap();
    assert_eq!(
        (next.outcome, next.run.as_str()),
        (Outcome::Created, "run-000002")
    );
    c.cancel(&project, &next.run, "crash-cancel-2").unwrap();
    wait_for(&c, &project, &next.run, RunState::is_final);
    assert_eq!(lab.starts(), 2);
}
