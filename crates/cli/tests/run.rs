//! `fh run` and the browser and stop options of `fh serve`. Each test has its own state
//! folder and stops its daemon at the end. Each command gets a fake browser program in
//! `BROWSER` and `FH_NO_OPEN=1`, so that no test opens the browser of the machine.
//!
//! The ccx is a wrapper around `fixtures/ccx/fake-ccx.sh`. One test uses the real ccx when it
//! is installed.
mod common;
use common::{ccx_solver, fails, p, root, scratch, script, stderr, stdout};

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A fake ccx: it adds a line to `LOG`. A model with the node set `SLOW` runs until a kill,
/// other models get zero results from `fixtures/ccx/fake-ccx.sh`.
const FAKE_SOLVER: &str = r#"#!/bin/sh
echo ccx >> LOG
if grep -q SLOW job.inp; then exec sleep 30; fi
exec FAKE "$@"
"#;

/// A scratch folder with a state folder, a fake browser and a fake ccx.
struct Lab {
    dir: PathBuf,
    state: PathBuf,
    solver: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = scratch(name);
        script(
            &dir.join("browser"),
            &format!(
                "#!/bin/sh\necho \"$1\" >> {}\n",
                p(&dir.join("browser.log"))
            ),
        );
        let fake = root().join("fixtures/ccx/fake-ccx.sh");
        let solver = script(
            &dir.join("fake-ccx"),
            &FAKE_SOLVER
                .replace("FAKE", p(&fake))
                .replace("LOG", p(&dir.join("ccx.log"))),
        );
        Lab {
            state: dir.join("state"),
            dir,
            solver,
        }
    }

    /// `fh ARGS --state STATE` with the fake browser, `FH_NO_OPEN=1` and the fake ccx.
    fn cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fh"));
        cmd.args(args).args(["--state", p(&self.state)]);
        cmd.current_dir(&self.dir)
            .env("BROWSER", self.dir.join("browser"))
            .env("FH_NO_OPEN", "1")
            .env("FH_CCX", &self.solver);
        cmd
    }

    fn fh(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("run fh")
    }

    fn input(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.join(name);
        fs::write(&path, text).unwrap();
        path
    }

    /// A ccx that prints `message` and fails.
    fn failing_solver(&mut self, message: &str) {
        let text = format!("#!/bin/sh\necho \"{message}\"\nexit 201\n");
        self.solver = script(&self.dir.join("failing-ccx"), &text);
    }

    fn browser_log(&self) -> Option<String> {
        fs::read_to_string(self.dir.join("browser.log")).ok()
    }

    /// The browser log when it has `n` lines. The browser program starts after `fh` ends.
    fn opened(&self, n: usize) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let log = self.browser_log().unwrap_or_default();
            if log.lines().count() >= n && log.ends_with('\n') {
                return log;
            }
            assert!(
                Instant::now() < deadline,
                "the browser did not open: {log:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn daemon_pid(&self) -> String {
        let info = fs::read_to_string(self.state.join("daemon.info")).unwrap();
        info.lines()
            .find_map(|l| l.strip_prefix("pid="))
            .unwrap()
            .to_owned()
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = self.fh(&["serve", "--stop"]);
    }
}

#[track_caller]
fn succeeds(out: &Output) {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}\n{}",
        stdout(out),
        stderr(out)
    );
}

/// True when the fake ccx of the lab ran.
fn solved(lab: &Lab) -> bool {
    lab.dir.join("ccx.log").is_file()
}

/// The tet model of the fixtures with the node set `SLOW`: the fake ccx runs until a kill.
fn slow_model(lab: &Lab) -> PathBuf {
    let tet = fs::read_to_string(root().join("fixtures/ccx/tet.inp")).unwrap();
    lab.input(
        "slow.inp",
        &tet.replace("*STEP", "*NSET, NSET=SLOW\n1\n*STEP"),
    )
}

#[test]
fn an_inp_file_runs_on_a_new_daemon_and_a_second_run_uses_it() {
    let lab = Lab::new("run-inp");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    let out = lab.fh(&["run", p(&bar)]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("fh run: started a daemon"), "{text}");
    assert!(text.contains("run-000001 succeeded"), "{text}");
    assert!(solved(&lab), "{}", stdout(&out));
    let folder = text
        .lines()
        .find_map(|l| l.strip_prefix("fh run: results in "))
        .unwrap();
    assert!(Path::new(folder).join("results.pvd").is_file());
    assert!(text.contains("&run=run-000001"), "{text}");
    // FH_NO_OPEN=1: no browser.
    assert_eq!(lab.browser_log(), None);
    // The daemon that fh run starts writes its start lines to daemon.log.
    let log = fs::read_to_string(lab.state.join("daemon.log")).unwrap();
    assert!(log.contains("fh serve: state folder"), "{log}");

    let pid = lab.daemon_pid();
    let again = lab.fh(&["run", p(&bar)]);
    succeeds(&again);
    assert!(!stdout(&again).contains("started a daemon"));
    assert!(stdout(&again).contains("run-000002 succeeded"));
    assert_eq!(lab.daemon_pid(), pid);
}

#[test]
fn the_daemon_log_has_mode_0600_for_an_old_and_a_new_log() {
    let lab = Lab::new("run-log-mode");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    let log = lab.state.join("daemon.log");
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    fs::create_dir_all(&lab.state).unwrap();
    fs::write(&log, "old\n").unwrap();
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644)).unwrap();
    succeeds(&lab.fh(&["run", p(&bar)]));
    assert_eq!(mode(&log), 0o600);
    succeeds(&lab.fh(&["serve", "--stop"]));
    fs::remove_file(&log).unwrap();
    succeeds(&lab.fh(&["run", p(&bar)]));
    assert_eq!(mode(&log), 0o600);
}

#[test]
fn a_relative_state_folder_names_the_folder_in_the_current_folder() {
    let lab = Lab::new("run-relative-state");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    // The last --state wins: "state" in the folder of the lab, the same as lab.state.
    let out = lab
        .cmd(&["run", p(&bar)])
        .args(["--state", "state"])
        .output();
    let out = out.unwrap();
    succeeds(&out);
    let state = fs::canonicalize(&lab.state).unwrap();
    let hint = format!("Stop it with: fh serve --stop --state {}\n", p(&state));
    assert!(stdout(&out).contains(&hint), "{}", stdout(&out));
    assert!(lab.state.join("daemon.info").is_file());
}

#[test]
fn two_runs_that_start_at_the_same_time_use_one_daemon() {
    let lab = Lab::new("run-race");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    let children: Vec<Child> = (0..2)
        .map(|_| {
            lab.cmd(&["run", p(&bar)])
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let outs: Vec<Output> = children
        .into_iter()
        .map(|c| c.wait_with_output().unwrap())
        .collect();
    for out in &outs {
        succeeds(out);
    }
    let started = outs
        .iter()
        .filter(|o| stdout(o).contains("started a daemon"))
        .count();
    assert_eq!(started, 1);
    let runs = outs
        .iter()
        .filter(|o| stdout(o).contains("succeeded in"))
        .count();
    assert_eq!(runs, 2);
    // The fh serve of the other fh run ended with its fh run. Thus after a stop, no daemon
    // takes the free lock, also after the 1 s in which a new daemon tries the lock.
    let stop = lab.fh(&["serve", "--stop"]);
    let said = format!("{}{}", stdout(&stop), stderr(&stop));
    assert!(said.contains("stopped the daemon"), "{said}");
    std::thread::sleep(Duration::from_millis(1500));
    let again = lab.fh(&["serve", "--stop"]);
    let said = format!("{}{}", stdout(&again), stderr(&again));
    assert!(said.contains("no daemon answers"), "{said}");
}

#[test]
fn the_browser_opens_on_the_run_unless_no_open_is_given() {
    let lab = Lab::new("run-open");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    let out = lab
        .cmd(&["run", p(&bar)])
        .env_remove("FH_NO_OPEN")
        .output()
        .unwrap();
    succeeds(&out);
    assert!(stdout(&out).contains("opened run-000001 in the browser"));
    // The browser gets the page that sends it on, not the address with the token.
    let opened = lab.opened(1);
    let state = fs::canonicalize(&lab.state).unwrap();
    assert_eq!(opened.trim(), p(&state.join("open.html")));
    let page = fs::read_to_string(opened.trim()).unwrap();
    let info = fs::read_to_string(lab.state.join("daemon.info")).unwrap();
    let token = info.lines().find_map(|l| l.strip_prefix("token=")).unwrap();
    assert!(
        page.contains(&format!("?token={token}&amp;run=run-000001\"")),
        "{page}"
    );

    let mut no_open = lab.cmd(&["run", p(&bar), "--no-open"]);
    succeeds(&no_open.env_remove("FH_NO_OPEN").output().unwrap());
    assert_eq!(lab.browser_log().unwrap().lines().count(), 1);
}

#[test]
fn a_missing_file_exits_2() {
    let lab = Lab::new("run-missing");
    let out = lab.fh(&["run", "no-such-model.inp"]);
    fails(&out, 2, "no-such-model.inp: ");
    fails(&out, 2, "(run.input.missing)");
    assert!(!lab.state.exists(), "no daemon for a missing file");
}

#[test]
fn the_timeout_option_stops_a_slow_solve() {
    let lab = Lab::new("run-timeout");
    let model = slow_model(&lab);
    let start = Instant::now();
    let out = lab.fh(&["run", p(&model), "--timeout", "1"]);
    fails(&out, 1, "run-000001 failed: ");
    assert!(start.elapsed() < Duration::from_secs(25));
}

#[test]
fn an_include_file_that_is_missing_exits_1_and_names_the_file() {
    let lab = Lab::new("run-no-include");
    let bad = lab.input("bad.inp", "*INCLUDE, INPUT=nope.inp\n");
    let out = lab.fh(&["run", p(&bad)]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("nope.inp"), "{}", stderr(&out));
    assert!(!solved(&lab), "ccx does not start");
}

#[test]
fn an_include_file_is_solved_with_the_input_and_a_later_edit_does_not_change_the_run() {
    let lab = Lab::new("run-include");
    let tet = fs::read_to_string(root().join("fixtures/ccx/tet.inp")).unwrap();
    let (mesh, step) = tet.split_at(tet.find("*STEP").unwrap());
    lab.input("mesh.inp", mesh);
    let job = lab.input("job.inp", &format!("*INCLUDE, INPUT=mesh.inp\n{step}"));
    succeeds(&lab.fh(&["run", p(&job)]));
    let kept = lab.state.join("runs/run-000001/in");
    assert_eq!(fs::read_to_string(kept.join("mesh.inp")).unwrap(), mesh);
    fs::write(lab.dir.join("mesh.inp"), "changed").unwrap();
    assert_eq!(fs::read_to_string(kept.join("mesh.inp")).unwrap(), mesh);
}

#[test]
fn a_run_that_fails_in_the_solve_exits_1() {
    let mut lab = Lab::new("run-failed");
    lab.failing_solver(" *ERROR in e_c3d: nonpositive jacobian");
    let model = root().join("validation/cases/bar-pressure/job.inp");
    let out = lab.fh(&["run", p(&model)]);
    fails(&out, 1, "run-000001 failed: ");
    fails(&out, 1, "*ERROR in e_c3d: nonpositive jacobian");
}

#[test]
fn no_daemon_possible_exits_3() {
    let lab = Lab::new("run-no-daemon");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    // The state folder cannot be made.
    fs::write(&lab.state, "not a folder").unwrap();
    let out = lab.fh(&["run", p(&bar)]);
    fails(&out, 3, "no daemon: cannot make the state folder");
    fs::remove_file(&lab.state).unwrap();

    // A daemon holds the lock of the state folder, but it does not answer.
    fs::create_dir_all(&lab.state).unwrap();
    let lock = fs::File::create(lab.state.join("daemon.lock")).unwrap();
    lock.try_lock().unwrap();
    let out = lab.fh(&["run", p(&bar)]);
    fails(&out, 3, "no daemon: fh serve did not start a daemon");
    fails(&out, 3, "another daemon uses this state folder");
}

/// The lines of the stdout of a child process.
type Lines = std::io::Lines<BufReader<std::process::ChildStdout>>;

/// Start `cmd` with its stdout read line by line.
fn start(cmd: &mut Command) -> (Child, Lines) {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let lines = BufReader::new(child.stdout.take().unwrap()).lines();
    (child, lines)
}

/// Read lines until a line holds `want`.
#[track_caller]
fn read_until(lines: &mut Lines, want: &str) {
    let mut seen = Vec::new();
    for line in lines.by_ref() {
        let line = line.unwrap();
        if line.contains(want) {
            return;
        }
        seen.push(line);
    }
    panic!("no line with {want}: {seen:?}");
}

/// Read the rest of the lines in a thread, so that the child never writes to a full or
/// closed pipe, and wait for the end of the child.
fn finish(child: Child, lines: Lines) -> Output {
    std::thread::spawn(move || lines.for_each(drop));
    child.wait_with_output().unwrap()
}

fn interrupt(child: &Child) {
    let sent = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status();
    assert!(sent.unwrap().success());
}

fn state_of(lab: &Lab, run: &str) -> String {
    let record = fs::read_to_string(lab.state.join("runs").join(run).join("record")).unwrap();
    record
        .lines()
        .find_map(|l| l.strip_prefix("state="))
        .unwrap()
        .to_owned()
}

#[test]
fn ctrl_c_cancels_the_run_through_the_contract() {
    let lab = Lab::new("run-cancel");
    let model = slow_model(&lab);
    let (child, mut lines) = start(&mut lab.cmd(&["run", p(&model)]));
    read_until(&mut lines, "  running");
    interrupt(&child);
    read_until(&mut lines, "  cancel: cancelling");
    let out = finish(child, lines);
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
    assert!(stderr(&out).contains("run-000001 was cancelled"));
    assert_eq!(state_of(&lab, "run-000001"), "cancelled");
    // The daemon stays alive after the Ctrl+C.
    let stop = lab.fh(&["serve", "--stop"]);
    assert!(
        stdout(&stop).contains("stopped the daemon"),
        "{}",
        stdout(&stop)
    );
}

#[test]
fn preview_shows_the_model_in_the_page_and_the_run_solves_the_same_model() {
    let lab = Lab::new("run-preview");
    let model = root().join("validation/cases/bar-pressure/job.inp");
    let out = lab.fh(&["run", p(&model), "--preview"]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(
        text.contains("fh run: preview: the page shows job.inp (preview 1)"),
        "{text}"
    );
    assert!(text.ends_with("fh run: --preview: no solve\n"), "{text}");
    // The chat of the page gets the entry `show <preview> <name>`.
    let c = fh_daemon::Client::discover(&lab.state).unwrap();
    let chat = c
        .send("GET", "/api/v1/chat?since=0", None, Duration::ZERO)
        .unwrap();
    let show = chat.items.iter().find_map(|i| i.split_once(" show "));
    assert_eq!(show.map(|s| s.1), Some("1 job.inp"), "{chat:?}");
    assert!(c.runs().unwrap().is_empty(), "a preview solves nothing");

    succeeds(&lab.fh(&["run", p(&model)]));
    let shown = fs::read(lab.state.join("previews/1/input.inp")).unwrap();
    let solved = fs::read(lab.state.join("runs/run-000001/in/input.inp")).unwrap();
    assert!(shown == solved, "the run solves the model of the preview");
}

#[test]
fn an_inp_file_runs_through_the_real_ccx() {
    let Some(solver) = ccx_solver() else { return };
    let lab = Lab::new("run-ccx");
    let bar = root().join("validation/cases/bar-pressure/job.inp");
    let run = |model: &Path| {
        lab.cmd(&["run", p(model)])
            .env("FH_CCX", &solver)
            .output()
            .unwrap()
    };
    // The model asks for text results only, so the page has no field to show.
    let out = run(&bar);
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("run-000001 succeeded"), "{text}");
    assert!(text.contains("the page has no field to show"), "{text}");
    assert!(!solved(&lab), "the real ccx solves, not the fake");
    let folder = text
        .lines()
        .find_map(|l| l.strip_prefix("fh run: results in "))
        .unwrap();
    assert!(Path::new(folder).join("ccx/job.dat").is_file());

    // With *NODE FILE the page gets a VTU file for every field that ccx writes.
    let job = fs::read_to_string(&bar).unwrap();
    let job = job.replace("*END STEP", "*NODE FILE\nU, RF\n*END STEP");
    let out = run(&lab.input("job.inp", &job));
    succeeds(&out);
    let text = stdout(&out);
    assert!(!text.contains("the page has no field"), "{text}");
    let folder = text
        .lines()
        .find_map(|l| l.strip_prefix("fh run: results in "))
        .unwrap();
    assert!(Path::new(folder).join("step-1.vtu").is_file());
}

#[test]
fn a_target_that_is_not_an_inp_file_exits_2() {
    let lab = Lab::new("run-not-inp");
    let other = lab.input("notes.txt", "text");
    fails(&lab.fh(&["run", p(&other)]), 2, "(run.input.kind)");
    let case = root().join("validation/cases/bar-pressure");
    fails(&lab.fh(&["run", p(&case)]), 2, "(run.input.kind)");
    fails(
        &lab.fh(&["run", p(&case.join("check.toml"))]),
        2,
        "(run.input.kind)",
    );
    assert!(!lab.state.exists(), "no daemon for a bad input");
}

#[test]
fn bad_run_arguments_exit_2() {
    let lab = Lab::new("run-usage");
    fails(&lab.fh(&["run"]), 2, "usage: fh run");
    // The lab adds --state, so this has no state folder. The check comes before its use.
    fails(
        &common::fh(&["run", "a.inp", "--timeout"]),
        2,
        "--timeout needs a number of seconds",
    );
    fails(
        &lab.fh(&["run", "a.inp", "--timeout", "0"]),
        2,
        "--timeout needs a number above 0",
    );
    fails(
        &lab.fh(&["run", "a.inp", "--memory", "x"]),
        2,
        "--memory needs a number above 0",
    );
    fails(
        &lab.fh(&["run", "a.inp", "--bogus"]),
        2,
        "unknown option: --bogus",
    );
    for gone in ["--size", "--order", "--units", "--case"] {
        fails(
            &lab.fh(&["run", "a.inp", gone, "1"]),
            2,
            &format!("unknown option: {gone}"),
        );
    }
    assert!(!lab.state.exists(), "no daemon for a usage error");
}

/// Start `fh serve` and read its output to the line that ends the start.
fn serve(lab: &Lab, args: &[&str], open: bool) -> (Child, Lines) {
    let mut cmd = lab.cmd(&[&["serve"], args].concat());
    if open {
        cmd.env_remove("FH_NO_OPEN");
    }
    let (child, mut lines) = start(&mut cmd);
    read_until(&mut lines, "Stop with Ctrl+C.");
    (child, lines)
}

#[test]
fn fh_serve_opens_the_page_unless_told_not_to_and_stop_ends_it() {
    let lab = Lab::new("serve-open");
    let (child, mut lines) = serve(&lab, &[], true);
    read_until(&mut lines, "fh serve: opened the page in the browser");
    lab.opened(1);
    let page = fs::read_to_string(lab.state.join("open.html")).unwrap();
    let info = fs::read_to_string(lab.state.join("daemon.info")).unwrap();
    let token = info.lines().find_map(|l| l.strip_prefix("token=")).unwrap();
    assert!(page.contains(&format!("/?token={token}\"")), "{page}");
    let stop = lab.fh(&["serve", "--stop"]);
    succeeds(&stop);
    assert!(stdout(&stop).contains("stopped the daemon"));
    let out = finish(child, lines);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!lab.state.join("daemon.info").exists());

    // --no-open, and FH_NO_OPEN.
    for (args, open) in [(&["--no-open"][..], true), (&[][..], false)] {
        let (child, lines) = serve(&lab, args, open);
        succeeds(&lab.fh(&["serve", "--stop"]));
        finish(child, lines);
    }
    assert_eq!(lab.browser_log().unwrap().lines().count(), 1);
}

#[test]
fn fh_serve_stop_with_no_daemon_and_with_other_options() {
    let lab = Lab::new("serve-stop");
    let out = lab.fh(&["serve", "--stop"]);
    succeeds(&out);
    assert!(
        stdout(&out).contains("no daemon answers"),
        "{}",
        stdout(&out)
    );
    fails(
        &lab.fh(&["serve", "--stop", "--port", "1"]),
        2,
        "--stop takes only --state DIR",
    );
}

#[test]
fn a_second_fh_serve_names_the_page_of_the_running_daemon() {
    let lab = Lab::new("serve-second");
    let (child, lines) = serve(&lab, &[], false);
    let second = lab.fh(&["serve"]);
    fails(&second, 1, "(daemon.running)");
    fails(&second, 1, "its page is http://127.0.0.1:");
    succeeds(&lab.fh(&["serve", "--stop"]));
    finish(child, lines);
}
