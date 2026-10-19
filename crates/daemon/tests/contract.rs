//! The D1 exit checks of the run contract, with a shell script as a fake `ccx`.
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

use fh_daemon::{Client, Config, Daemon, Limits, Options, RunRecord, RunState};
use fh_solver_ccx::Ccx;

/// A fake `ccx` that runs until a kill when the model has the node set `SLOW`. Other models
/// wait for the file `gate` and then get results from `fixtures/ccx/fake-ccx.sh`.
const SOLVER: &str = r#"#!/bin/sh
echo start >> DIR/started.log
if grep -q SLOW job.inp; then
  echo $$ > DIR/solver.pid
  exec sleep 30
fi
while [ ! -e DIR/gate ]; do sleep 0.02; done
exec FAKE "$@"
"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/ccx")
        .join(name)
}

/// A model with one C3D10 tet that `ccx` can solve.
fn tet() -> String {
    fs::read_to_string(fixture("tet.inp")).unwrap()
}

/// The tet model with the node set `SLOW`.
fn slow() -> String {
    tet().replace("*STEP", "*NSET, NSET=SLOW\n1\n*STEP")
}

fn ccx(exe: &Path) -> Ccx {
    Ccx {
        exe: exe.into(),
        ..Ccx::new()
    }
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Wait until the process `pid` has ended.
fn ended(pid: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(pid) {
        assert!(Instant::now() < deadline, "ccx {pid} is still alive");
        std::thread::sleep(Duration::from_millis(20));
    }
}

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
        let text = SOLVER.replace("DIR", dir.to_str().unwrap());
        let text = text.replace("FAKE", fixture("fake-ccx.sh").to_str().unwrap());
        fs::write(&solver, text).unwrap();
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
            solver: ccx(&self.solver),
        }
    }

    fn start(&self) -> Daemon {
        Daemon::start_with_options(self.config(), Options::default()).unwrap()
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
    let input = lab.input("a.inp", &tet());

    let foreign = daemon.client().with_origin("http://evil.example");
    let e = foreign.submit(&input, &Limits::default()).unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (403, "origin.refused"));
    assert!(
        cli.runs().unwrap().is_empty(),
        "a refused submit made a run"
    );

    let own = daemon
        .client()
        .with_origin(&format!("http://127.0.0.1:{}", daemon.port()));
    let run = own.submit(&input, &Limits::default()).unwrap().run;
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
    let input = lab.input("a.inp", &tet());
    let run = browser.submit(&input, &Limits::default()).unwrap().run;
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
    let run = c
        .submit(&lab.input("slow.inp", &slow()), &Limits::default())
        .unwrap()
        .run;
    let waiting = c
        .submit(&lab.input("b.inp", &tet()), &Limits::default())
        .unwrap()
        .run;
    wait_for(&c, &run, |s| s == RunState::Running);
    let pid = wait_file(&lab, "solver.pid");
    assert_eq!(c.record(&waiting).unwrap().state, RunState::Queued);

    // A queued run ends at once and never starts.
    assert_eq!(c.cancel(&waiting).unwrap().state, RunState::Cancelled);
    assert_eq!(c.cancel(&run).unwrap().state, RunState::Cancelling);
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Cancelled);
    ended(&pid);
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
    let input = lab.input("a.inp", &tet());
    let first = c.submit(&input, &Limits::default()).unwrap().run;
    // The user changes the file while the run waits.
    lab.input("a.inp", &slow());
    let copy = lab.state.join("runs").join(&first).join("in/input.inp");
    assert_eq!(fs::read_to_string(&copy).unwrap(), tet());
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    let record = c.record(&first).unwrap();
    assert_eq!(record.input, input.to_str().unwrap());

    let relative = c
        .submit(Path::new("a.inp"), &Limits::default())
        .unwrap_err();
    assert_eq!(relative.code, "input.invalid");
    lab.open_gate();
    let record = wait_for(&c, &first, RunState::is_final);
    assert_eq!(record.state, RunState::Succeeded, "{}", record.message);
}

#[test]
fn the_limits_of_a_run_stop_a_slow_solve() {
    let lab = Lab::new("limits");
    let daemon = lab.start();
    let c = daemon.client();
    let slow = lab.input("slow.inp", &slow());
    let limits = Limits {
        timeout: Some(Duration::from_secs(1)),
        ..Limits::default()
    };
    let run = c.submit(&slow, &limits).unwrap().run;
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Failed, "{}", record.message);
    assert!(record.message.contains("time limit"), "{}", record.message);
}

#[test]
fn a_second_daemon_cannot_use_the_same_state() {
    let lab = Lab::new("lock");
    let _daemon = lab.start();
    let e = Daemon::start_with_options(lab.config(), Options::default())
        .err()
        .unwrap();
    assert_eq!(e.code, "daemon.running");
}

#[test]
fn a_stopped_daemon_leaves_interrupted_records() {
    let lab = Lab::new("stop");
    let daemon = lab.start();
    let c = daemon.client();
    let run = c
        .submit(&lab.input("slow.inp", &slow()), &Limits::default())
        .unwrap()
        .run;
    let waiting = c
        .submit(&lab.input("b.inp", &tet()), &Limits::default())
        .unwrap()
        .run;
    wait_for(&c, &run, |s| s == RunState::Running);
    let pid = wait_file(&lab, "solver.pid");
    let old_token = daemon.token().to_owned();
    drop(daemon);
    assert!(!alive(&pid), "ccx lives after the daemon stopped");

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
        solver: ccx(Path::new(&solver)),
    };
    let _daemon = Daemon::start_with_options(config, Options::default()).unwrap();
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
    let slow = lab.input("slow.inp", &slow());
    let run = c.submit(&slow, &Limits::default()).unwrap().run;
    wait_for(&c, &run, |s| s == RunState::Running);
    let pid = wait_file(&lab, "solver.pid");

    // SIGKILL: the daemon cannot clean up.
    child.kill().unwrap();
    child.wait().unwrap();
    // The pipe to the guard closes, and the guard kills the group of ccx.
    ended(&pid);

    // The next daemon shows the run as interrupted, not as running.
    let daemon = lab.start();
    let c = daemon.client();
    assert_eq!(c.record(&run).unwrap().state, RunState::Interrupted);
    assert_eq!(c.runs().unwrap(), [(run.clone(), RunState::Interrupted)]);
    // The run counter goes on. No run ID is used twice.
    fs::remove_file(lab.dir.join("solver.pid")).unwrap();
    let next = c.submit(&slow, &Limits::default()).unwrap().run;
    assert_eq!(next, "run-000002");
    let pid = wait_file(&lab, "solver.pid");
    c.cancel(&next).unwrap();
    wait_for(&c, &next, RunState::is_final);
    ended(&pid);
    assert_eq!(lab.starts(), 2);
}

#[test]
fn the_last_event_of_a_failed_run_tells_why() {
    let lab = Lab::new("failed-event");
    fs::write(&lab.solver, "#!/bin/sh\necho boom\nexit 1\n").unwrap();
    let daemon = lab.start();
    let c = daemon.client();
    let run = c
        .submit(&lab.input("a.inp", &tet()), &Limits::default())
        .unwrap()
        .run;
    let record = wait_for(&c, &run, RunState::is_final);
    assert_eq!(record.state, RunState::Failed);
    assert!(record.message.contains("boom"), "{}", record.message);
    let events = c.events(&run, 0, Duration::ZERO).unwrap();
    let last = events.items.last().map(|e| e.text.clone());
    assert_eq!(last, Some(format!("state failed: {}", record.message)));
}
