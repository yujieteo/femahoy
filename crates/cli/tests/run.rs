//! `fh run` and the browser and stop options of `fh serve`. Each test has its own state
//! folder and stops its daemon at the end. Each command gets a fake browser program in
//! `BROWSER` and `FH_NO_OPEN=1`, so that no test opens the browser of the machine.
//!
//! The fake gmsh copies `fixtures/msh/bar-pressure.msh`, the output of gmsh 4.15.2 for
//! `fh mesh validation/cases/bar-pressure/bar.step --case validation/cases/bar-pressure`.
//! Thus the case tests run with no gmsh. One test uses the real gmsh when it is installed.
//! The solver is a fake protocol v2 solver. One test uses the real ccx when it is installed.
mod common;
use common::{ccx_solver, fails, p, real_gmsh, root, scratch, script, stderr, stdout};

use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A protocol v2 solver that writes one step with no data and a summary. It does not read the
/// input.
const FAKE_SOLVER: &str = r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=1"
echo "fake solve done"
echo '<VTKFile type="Collection"><Collection><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>' > results.pvd
echo '<VTKFile/>' > step-1.vtu
printf 'schema: fh.run\nversion: 1.1.0\nstatus: ok\n' > summary.toon
"#;

/// A protocol v2 solver that waits for a cancel. With `stubborn` in the input, it waits for
/// the end of stdin only.
const SLOW_SOLVER: &str = r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=0.1"
if grep -q stubborn "$1"; then
  while read line; do :; done
  exit 130
fi
while read line; do
  if [ "$line" = cancel ]; then exit 130; fi
done
exit 130
"#;

/// A scratch folder with a state folder, a fake browser and a fake gmsh.
struct Lab {
    dir: PathBuf,
    state: PathBuf,
    solver: PathBuf,
    gmsh: PathBuf,
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
        let fixture = root().join("fixtures/msh/bar-pressure.msh");
        let gmsh = script(
            &dir.join("gmsh"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = -version ]; then echo 4.15.2; exit 0; fi\n\
                 while [ $# -gt 1 ]; do if [ \"$1\" = -o ]; then out=\"$2\"; fi; shift; done\n\
                 cp {} \"$out\"\n",
                p(&fixture)
            ),
        );
        let solver = script(&dir.join("fake-solver"), FAKE_SOLVER);
        Lab {
            state: dir.join("state"),
            dir,
            solver,
            gmsh,
        }
    }

    /// `fh ARGS --state STATE` with the fake browser, `FH_NO_OPEN=1` and the fake gmsh.
    fn cmd(&self, args: &[&str]) -> Command {
        self.cmd_of(Path::new(env!("CARGO_BIN_EXE_fh")), args)
    }

    /// [`Lab::cmd`] for the `fh` program at `fh`.
    fn cmd_of(&self, fh: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(fh);
        cmd.args(args).args(["--state", p(&self.state)]);
        cmd.current_dir(&self.dir)
            .env("BROWSER", self.dir.join("browser"))
            .env("FH_NO_OPEN", "1")
            .env("FH_SOLVE", &self.solver)
            .env("FH_GMSH", &self.gmsh);
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

    fn slow_solver(&mut self) {
        self.solver = script(&self.dir.join("slow-solver"), SLOW_SOLVER);
    }

    /// A solver that prints the header, then `message` on stderr, and fails.
    fn failing_solver(&mut self, message: &str) {
        let text =
            format!("#!/bin/sh\necho \"fh-solver-protocol 2\"\necho \"{message}\" >&2\nexit 1\n");
        self.solver = script(&self.dir.join("failing-solver"), &text);
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

/// True when the fake solver ran: its log line is in the output of `fh run`.
fn solved(out: &Output) -> bool {
    stdout(out).contains("\n  fake solve done\n")
}

#[test]
fn an_inp_file_runs_on_a_new_daemon_and_a_second_run_uses_it() {
    let lab = Lab::new("run-inp");
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
    let out = lab.fh(&["run", p(&bar)]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("fh run: started a daemon"), "{text}");
    assert!(text.contains("run-000001 succeeded"), "{text}");
    assert!(text.contains("  schema: fh.run"), "{text}");
    assert!(solved(&out), "{}", stdout(&out));
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
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
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
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
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
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
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
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
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
fn an_input_that_the_solver_refuses_exits_1_with_the_line_in_the_file() {
    let mut lab = Lab::new("run-refused");
    // The error of fh_inp, as fh-solve-ccx prints it, with the path that the solver gets.
    lab.failing_solver("$1: line 3, *BOGUS: unsupported keyword (INP002)");
    let bad = lab.input("bad.inp", "*NODE\n1, 0, 0\n*BOGUS\n");
    let out = lab.fh(&["run", p(&bad)]);
    fails(&out, 1, "run-000001 failed: ");
    // The message names the file of the user, not the copy in the state folder.
    let bad = fs::canonicalize(bad).unwrap();
    fails(
        &out,
        1,
        &format!("{}: line 3, *BOGUS: unsupported keyword (INP002)", p(&bad)),
    );
}

#[test]
fn a_run_that_fails_in_the_solve_exits_1() {
    let mut lab = Lab::new("run-failed");
    lab.failing_solver("ccx failed: singular stiffness");
    let model = root().join("validation/cases/cantilever-c3d10/mesh-8.inp");
    let out = lab.fh(&["run", p(&model)]);
    fails(&out, 1, "run-000001 failed: ");
    fails(&out, 1, "ccx failed: singular stiffness");
}

#[test]
fn no_daemon_possible_exits_3() {
    let lab = Lab::new("run-no-daemon");
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
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
    let mut lab = Lab::new("run-cancel");
    lab.slow_solver();
    let model = lab.input("slow.inp", "slow\n");
    let (child, mut lines) = start(&mut lab.cmd(&["run", p(&model)]));
    read_until(&mut lines, "step 1/1 solve 10%");
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
fn a_second_ctrl_c_ends_fh_run_at_once_and_the_cancel_continues() {
    let mut lab = Lab::new("run-cancel-twice");
    lab.slow_solver();
    let model = lab.input("slow.inp", "stubborn\n");
    let (child, mut lines) = start(&mut lab.cmd(&["run", p(&model)]));
    read_until(&mut lines, "step 1/1 solve 10%");
    interrupt(&child);
    read_until(&mut lines, "  cancel: cancelling");
    interrupt(&child);
    read_until(&mut lines, "Ctrl+C again: fh run ends");
    let out = finish(child, lines);
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
    assert_eq!(state_of(&lab, "run-000001"), "cancelling");
    // The solver ignores the cancel. The runner stops it after its grace time.
    let deadline = Instant::now() + Duration::from_secs(20);
    while state_of(&lab, "run-000001") != "cancelled" {
        assert!(Instant::now() < deadline, "the cancel did not end the run");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_case_folder_runs_mesh_model_and_solve_and_does_not_change() {
    let lab = Lab::new("run-case");
    let case = root().join("validation/cases/bar-pressure");
    let before: Vec<_> = fs::read_dir(&case)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let committed = fs::read(case.join("bar.inp")).unwrap();
    let out = lab.fh(&["run", p(&case)]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("fh run: mesh: fh mesh "), "{text}");
    assert!(text.contains("fh run: model: fh model "), "{text}");
    assert!(solved(&out), "{}", stdout(&out));
    let work: Vec<_> = fs::read_dir(lab.state.join("cases")).unwrap().collect();
    assert_eq!(work.len(), 1);
    let work = work[0].as_ref().unwrap().path();
    assert!(work
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("bar-pressure-"));
    assert!(work.join("bar.msh").is_file() && work.join("bar.inp").is_file());
    let after: Vec<_> = fs::read_dir(&case)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(before.len(), after.len());
    assert_eq!(fs::read(case.join("bar.inp")).unwrap(), committed);

    // case.toml and the STEP file name the same case.
    succeeds(&lab.fh(&["run", p(&case.join("case.toml"))]));
    succeeds(&lab.fh(&["run", p(&case.join("bar.step"))]));
    assert_eq!(fs::read_dir(lab.state.join("cases")).unwrap().count(), 1);
}

#[test]
fn without_fh_solve_a_run_uses_fh_solve_ccx() {
    let lab = Lab::new("run-case-solver");
    // A copy of fh in its own folder: the solvers beside fh are the solvers of the test.
    let bin = lab.dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fh = bin.join("fh");
    fs::copy(env!("CARGO_BIN_EXE_fh"), &fh).unwrap();
    let case = root().join("validation/cases/bar-pressure");
    let dir = lab.dir.join("bar-ccx");
    fs::create_dir_all(&dir).unwrap();
    for file in ["bar.step", "derivation.md"] {
        fs::copy(case.join(file), dir.join(file)).unwrap();
    }
    let toml = fs::read_to_string(case.join("case.toml")).unwrap();
    let toml = toml.replace("\nmesh = ", "\nsolver = \"fh-solve-ccx\"\nmesh = ");
    fs::write(dir.join("case.toml"), toml).unwrap();
    let run = |path: &str| {
        let mut cmd = lab.cmd_of(&fh, &["run", p(&dir)]);
        cmd.env_remove("FH_SOLVE").env("PATH", path);
        cmd.output().unwrap()
    };

    // No fh-solve-ccx beside fh or on PATH: fh run stops before a daemon starts.
    let out = run("/usr/bin:/bin");
    fails(
        &out,
        1,
        "the solver fh-solve-ccx is not beside fh or on PATH",
    );
    fails(&out, 1, "docs/ccx.md (run.solver.missing)");
    assert!(!lab.state.join("daemon.info").exists());

    // fh-solve-ccx beside fh. It writes a line and runs the fake solver.
    let log = lab.dir.join("ccx.log");
    script(
        &bin.join("fh-solve-ccx"),
        &format!(
            "#!/bin/sh\necho ccx >> {}\nexec {} \"$@\"\n",
            p(&log),
            p(&lab.solver)
        ),
    );
    let out = run(&std::env::var("PATH").unwrap());
    succeeds(&out);
    assert!(solved(&out), "{}", stdout(&out));
    assert_eq!(fs::read_to_string(&log).unwrap(), "ccx\n");

    // FH_SOLVE wins over fh-solve-ccx.
    succeeds(&lab.fh(&["serve", "--stop"]));
    succeeds(&lab.fh(&["run", p(&dir)]));
    assert_eq!(fs::read_to_string(&log).unwrap(), "ccx\n");
}

#[test]
fn preview_shows_the_case_in_the_page_and_the_run_solves_the_same_model() {
    let lab = Lab::new("run-preview");
    let case = root().join("validation/cases/bar-pressure");
    let out = lab.fh(&["run", p(&case), "--preview"]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(
        text.contains("fh run: preview: the page shows bar-pressure (preview 1)"),
        "{text}"
    );
    assert!(text.ends_with("fh run: --preview: no solve\n"), "{text}");
    // The chat of the page gets the entry `show <preview> <name>`.
    let c = fh_daemon::Client::discover(&lab.state).unwrap();
    let chat = c
        .send("GET", "/api/v1/chat?since=0", None, Duration::ZERO)
        .unwrap();
    let show = chat.items.iter().find_map(|i| i.split_once(" show "));
    assert_eq!(show.map(|s| s.1), Some("1 bar-pressure"), "{chat:?}");
    assert!(c.runs().unwrap().is_empty(), "a preview solves nothing");

    succeeds(&lab.fh(&["run", p(&case)]));
    let shown = fs::read(lab.state.join("previews/1.inp")).unwrap();
    let solved = fs::read(lab.state.join("runs/run-000001/input.inp")).unwrap();
    assert!(shown == solved, "the run solves the model of the preview");
}

#[test]
fn an_inp_file_runs_through_the_real_ccx() {
    let Some(solver) = ccx_solver() else { return };
    let lab = Lab::new("run-ccx");
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
    let out = lab
        .cmd(&["run", p(&bar)])
        .env("FH_SOLVE", solver)
        .output()
        .unwrap();
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("run-000001 succeeded"), "{text}");
    assert!(text.contains("  status: ok"), "{text}");
    assert!(text.contains("  solver: \"ccx "), "{text}");
}

#[test]
fn a_folder_that_is_not_a_case_exits_2() {
    let lab = Lab::new("run-not-case");
    let case = root().join("validation/cases/bar-pressure");
    let empty = lab.dir.join("empty");
    fs::create_dir_all(&empty).unwrap();
    fails(&lab.fh(&["run", p(&empty)]), 2, "(run.case.missing)");

    let no_step = lab.dir.join("no-step");
    fs::create_dir_all(&no_step).unwrap();
    fs::copy(case.join("case.toml"), no_step.join("case.toml")).unwrap();
    fails(&lab.fh(&["run", p(&no_step)]), 2, "(run.case.step.missing)");

    fs::copy(case.join("bar.step"), no_step.join("a.step")).unwrap();
    fs::copy(case.join("bar.step"), no_step.join("b.STP")).unwrap();
    let out = lab.fh(&["run", p(&no_step)]);
    fails(&out, 2, "has 2 STEP files (a.step, b.STP)");
    fails(&out, 2, "(run.case.step.many)");

    let other = lab.input("notes.txt", "text");
    fails(&lab.fh(&["run", p(&other)]), 2, "(run.input.kind)");
    assert!(!lab.state.exists(), "no daemon for a bad input");
}

#[test]
fn a_case_that_the_mesh_step_refuses_exits_1() {
    let lab = Lab::new("run-case-refused");
    let case = root().join("validation/cases/bar-pressure");
    let dir = lab.dir.join("bar-m");
    fs::create_dir_all(&dir).unwrap();
    for file in ["bar.step", "derivation.md"] {
        fs::copy(case.join(file), dir.join(file)).unwrap();
    }
    let toml = fs::read_to_string(case.join("case.toml")).unwrap();
    fs::write(dir.join("case.toml"), toml.replace("SI-mm", "SI-m")).unwrap();
    let out = lab.fh(&["run", p(&dir)]);
    fails(&out, 1, "fh mesh: unit.step.mismatch");
}

#[test]
fn the_example_runs_from_any_folder() {
    let lab = Lab::new("run-example");
    let out = lab.fh(&["run", "--example", "bar-pressure"]);
    succeeds(&out);
    let text = stdout(&out);
    assert!(text.contains("fh run: example bar-pressure: "), "{text}");
    assert!(text.contains("--size 8"), "{text}");
    assert!(solved(&out), "{}", stdout(&out));
    let example = lab.state.join("examples/bar-pressure");
    for file in ["case.toml", "bar.step", "derivation.md"] {
        let shipped = fs::read(root().join("validation/cases/bar-pressure").join(file));
        assert_eq!(
            fs::read(example.join(file)).unwrap(),
            shipped.unwrap(),
            "{file}"
        );
    }
}

#[test]
fn the_example_runs_with_the_real_gmsh() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let lab = Lab::new("run-example-gmsh");
    let out = lab
        .cmd(&["run", "--example", "bar-pressure"])
        .env("FH_GMSH", gmsh)
        .output()
        .unwrap();
    succeeds(&out);
    assert!(stdout(&out).contains("nodes\t844"), "{}", stdout(&out));
    assert!(solved(&out), "{}", stdout(&out));
}

#[test]
fn bad_run_arguments_exit_2() {
    let lab = Lab::new("run-usage");
    fails(&lab.fh(&["run"]), 2, "usage: fh run");
    let unknown = lab.fh(&["run", "--example", "nothing"]);
    fails(
        &unknown,
        2,
        "unknown example nothing. The examples: bar-pressure",
    );
    // The lab adds --state, so this has no state folder. The check comes before its use.
    let out = common::fh(&["run", "--example"]);
    fails(&out, 2, "--example needs a name: bar-pressure");
    fails(
        &lab.fh(&["run", "a.inp", "--size", "-1"]),
        2,
        "--size needs a number above 0",
    );
    fails(
        &lab.fh(&["run", "a.inp", "--bogus"]),
        2,
        "unknown option: --bogus",
    );
    let bar = root().join("validation/cases/bar-pressure/bar.inp");
    for option in ["--size", "--order"] {
        let out = lab.fh(&["run", p(&bar), option, "1"]);
        fails(&out, 2, "(run.option.mesh)");
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
