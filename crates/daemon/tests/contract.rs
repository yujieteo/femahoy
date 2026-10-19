//! The D1 exit checks of the run contract, with a shell script as a protocol v2 solver.
//!
//! Browser close, cancel, a daemon stop and a daemon failure each keep one correct run
//! record. A foreign browser `Origin` fails. An authorized CLI passes.

use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use fh_daemon::{Client, Config, Daemon, Options, RunRecord, RunState};
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
        Daemon::start_with_options(self.config(), 0, Options::default()).unwrap()
    }

    fn input(&self, name: &str, text: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, text).unwrap();
        path
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

fn wait_for(c: &Client, run: &str, want: fn(RunState) -> bool) -> RunRecord {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut since = 0;
    loop {
        let events = c.events(run, since, Duration::from_millis(500)).unwrap();
        since = events.last;
        if want(events.state) {
            return c.record(run).unwrap();
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
fn send_and_close(port: u16, token: &str, origin: &str, head: &str) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "{head} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Origin: {origin}\r\nContent-Length: 0\r\n\r\n"
    )
    .unwrap();
}

#[test]
fn an_authorized_cli_passes_and_a_foreign_origin_fails() {
    let lab = Lab::new("origin");
    let daemon = lab.start();
    let cli = Client::discover(&lab.state).unwrap();
    assert_eq!(cli.port(), daemon.port());
    let input = lab.input("a.inp", "*NODE\n");

    let foreign = daemon.client().with_origin("http://evil.example");
    let e = foreign.submit(&input).unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (403, "origin.refused"));
    assert!(
        cli.runs().unwrap().is_empty(),
        "a refused submit made a run"
    );

    let own = daemon
        .client()
        .with_origin(&format!("http://127.0.0.1:{}", daemon.port()));
    let run = own.submit(&input).unwrap().run;
    lab.open_gate();
    let record = wait_for(&cli, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded);
    // Only the user can read the token.
    let info = fs::metadata(lab.state.join(fh_daemon::INFO_FILE)).unwrap();
    assert_eq!(info.permissions().mode() & 0o777, 0o600);
}

#[test]
fn a_closed_browser_does_not_stop_the_run() {
    let lab = Lab::new("browser");
    let daemon = lab.start();
    let page = format!("http://127.0.0.1:{}", daemon.port());
    let browser = daemon.client().with_origin(&page);
    let input = lab.input("a.inp", "*NODE\n");
    let run = browser.submit(&input).unwrap().run;
    wait_for(&browser, &run, |s| s == RunState::Running);

    // The browser waits for events, and then the tab closes.
    let events = format!("GET /api/v1/runs/{run}/events?since=100&wait_ms=20000");
    send_and_close(daemon.port(), daemon.token(), &page, &events);
    std::thread::sleep(Duration::from_millis(100));
    lab.open_gate();

    // A client that follows the events over many connections sees each event once.
    let mut seen = Vec::new();
    let mut since = 0;
    loop {
        let events = Client::discover(&lab.state)
            .unwrap()
            .events(&run, since, Duration::from_secs(5))
            .unwrap();
        assert_eq!(events.dropped, 0);
        seen.extend(events.items.iter().map(|e| e.seq));
        since = events.last;
        if events.state.is_final() && events.items.is_empty() {
            assert_eq!(events.state, RunState::Succeeded);
            break;
        }
    }
    assert_eq!(seen, (1..=since).collect::<Vec<_>>());
    assert_eq!(lab.starts(), 1);
}

#[test]
fn cancel_keeps_one_record() {
    let lab = Lab::new("cancel");
    let daemon = lab.start();
    let c = daemon.client();
    let run = c.submit(&lab.input("slow.inp", "** slow\n")).unwrap().run;
    let waiting = c.submit(&lab.input("b.inp", "*NODE\n")).unwrap().run;
    wait_for(&c, &run, |s| s == RunState::Running);
    assert_eq!(c.record(&waiting).unwrap().state, RunState::Queued);

    // A queued run ends at once and never starts.
    assert_eq!(c.cancel(&waiting).unwrap().state, RunState::Cancelled);
    assert_eq!(c.cancel(&run).unwrap().state, RunState::Cancelling);
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Cancelled);
    assert_eq!(wait_file(&lab, "solver.end"), "cancelled\n");
    // A cancel of a final run changes nothing.
    assert_eq!(c.cancel(&run).unwrap(), record);
    assert_eq!(c.cancel("run-000009").unwrap_err().status, 404);
    assert_eq!(
        c.runs().unwrap(),
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
    assert_eq!(c.record(&run).unwrap(), record);
    let events = c.events(&run, 0, Duration::ZERO).unwrap();
    let texts: Vec<&str> = events.items.iter().map(|e| e.text.as_str()).collect();
    assert_eq!(texts.first(), Some(&"state queued"));
    assert_eq!(texts.last(), Some(&"state cancelled"));
}

#[test]
fn a_run_solves_a_copy_of_its_input() {
    let lab = Lab::new("copy");
    let daemon = lab.start();
    let c = daemon.client();
    let input = lab.input("a.inp", "*NODE\n1, 0, 0, 0\n");
    let first = c.submit(&input).unwrap().run;
    // The user changes the file while the run waits.
    lab.input("a.inp", "*NODE\n1, 9, 9, 9\n");
    let copy = lab.state.join("runs").join(&first).join("input.inp");
    assert_eq!(fs::read_to_string(&copy).unwrap(), "*NODE\n1, 0, 0, 0\n");
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    let record = c.record(&first).unwrap();
    assert_eq!(record.input, input.to_str().unwrap());

    let relative = c.submit(Path::new("a.inp")).unwrap_err();
    assert_eq!(relative.code, "input.invalid");
    lab.open_gate();
    wait_for(&c, &first, RunState::is_final);
}

#[test]
fn the_event_log_and_queue_are_bounded() {
    let lab = Lab::new("bounded");
    let daemon = lab.start();
    let c = daemon.client();
    lab.open_gate();
    let run = c.submit(&lab.input("a.inp", "** chatty\n")).unwrap().run;
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded, "{}", record.message);
    let events = c.events(&run, 0, Duration::ZERO).unwrap();
    assert!(events.dropped > 0 && events.last > fh_daemon::EVENT_QUEUE as u64);
    assert!(events
        .items
        .iter()
        .all(|e| e.text.len() <= fh_daemon::EVENT_TEXT));
    let log = lab.state.join("runs").join(&run).join("events.log");
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
    let e = Daemon::start_with_options(lab.config(), 0, Options::default())
        .err()
        .unwrap();
    assert_eq!(e.code, "daemon.running");
}

#[test]
fn a_stopped_daemon_leaves_interrupted_records() {
    let lab = Lab::new("stop");
    let daemon = lab.start();
    let c = daemon.client();
    let run = c.submit(&lab.input("slow.inp", "** slow\n")).unwrap().run;
    let waiting = c.submit(&lab.input("b.inp", "*NODE\n")).unwrap().run;
    wait_for(&c, &run, |s| s == RunState::Running);
    let old_token = daemon.token().to_owned();
    drop(daemon);
    assert_eq!(lab.file("solver.end").as_deref(), Some("cancelled\n"));

    let daemon = lab.start();
    let stale = Client::new(daemon.port(), old_token).status().unwrap_err();
    assert_eq!(stale.code, "token.invalid");
    let c = Client::discover(&lab.state).unwrap();
    assert_eq!(
        c.runs().unwrap(),
        [
            (run, RunState::Interrupted),
            (waiting, RunState::Interrupted)
        ]
    );
    assert_eq!(lab.starts(), 1);
}

/// The daemon of `a_failed_daemon_leaves_no_running_run`, in a child process.
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
    let _daemon = Daemon::start_with_options(config, 0, Options::default()).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn a_failed_daemon_leaves_no_running_run() {
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
    let slow = lab.input("slow.inp", "** slow\n");
    let run = c.submit(&slow).unwrap().run;
    wait_for(&c, &run, |s| s == RunState::Running);
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

    // The next daemon shows the run as interrupted, not as running.
    let daemon = lab.start();
    let c = daemon.client();
    assert_eq!(c.record(&run).unwrap().state, RunState::Interrupted);
    assert_eq!(c.runs().unwrap(), [(run.clone(), RunState::Interrupted)]);
    // The run counter goes on. No run ID is used twice.
    let next = c.submit(&slow).unwrap().run;
    assert_eq!(next, "run-000002");
    c.cancel(&next).unwrap();
    wait_for(&c, &next, RunState::is_final);
    assert_eq!(lab.starts(), 2);
}

#[test]
fn the_last_event_of_a_failed_run_tells_why() {
    let lab = Lab::new("failed-event");
    fs::write(
        &lab.solver,
        "#!/bin/sh\necho \"fh-solver-protocol 2\"\necho boom >&2\nexit 1\n",
    )
    .unwrap();
    let daemon = lab.start();
    let c = daemon.client();
    let run = c.submit(&lab.input("a.inp", "*NODE\n")).unwrap().run;
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Failed);
    assert!(record.message.contains("boom"), "{}", record.message);
    let events = c.events(&run, 0, Duration::ZERO).unwrap();
    let last = events.items.last().map(|e| e.text.clone());
    assert_eq!(last, Some(format!("state failed: {}", record.message)));
}
