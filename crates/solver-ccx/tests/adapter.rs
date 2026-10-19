//! The `ccx` run with small fake `ccx` scripts: the files that `ccx` gets, the errors, and the
//! stop of `ccx` by the cancel, the time limit and the memory limit.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fh_solver_ccx::{code, CancelToken, Ccx};

/// An empty scratch folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fh-ccx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fake `ccx`: a shell script that runs `body` in the `ccx` work folder.
fn fake_ccx(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join("ccx");
    std::fs::write(&exe, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

/// `ccx` at `exe` with the default limits.
fn ccx(exe: &Path) -> Ccx {
    Ccx {
        exe: exe.into(),
        ..Ccx::new()
    }
}

/// Solve to the end: the error text, or an empty text on success.
fn run(input: &Path, run: &Path, exe: &Path) -> Result<(), String> {
    ccx(exe).run(input, run).map_err(|e| e.to_string())
}

/// One C3D10 tet, tied at x = 0 and pulled at node 2, x = 1. Nodes 5 to 10 are the midside
/// nodes of the edges 1-2, 2-3, 3-1, 1-4, 2-4 and 3-4.
const TET: &str = "** units: SI-mm
*NODE
1, 0, 0, 0
2, 1, 0, 0
3, 0, 1, 0
4, 0, 0, 1
5, 0.5, 0, 0
6, 0.5, 0.5, 0
7, 0, 0.5, 0
8, 0, 0, 0.5
9, 0.5, 0, 0.5
10, 0, 0.5, 0.5
*ELEMENT, TYPE=C3D10
1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
*NSET, NSET=FIXED
1, 3, 4, 7, 8, 10
*MATERIAL, NAME=STEEL
*ELASTIC
210000, 0.3
*STEP, NAME=PULL
*STATIC
*BOUNDARY
FIXED, 1, 3
2, 1, 1, 0.001
*END STEP
";

fn tet(dir: &Path, extra: &str) -> PathBuf {
    let file = dir.join("tet.inp");
    std::fs::write(&file, TET.replace("*STEP", &format!("{extra}*STEP"))).unwrap();
    file
}

/// `ccx` gets the file as the person wrote it, and each file that it includes.
#[test]
fn ccx_gets_the_input_and_its_include_files_unchanged() {
    let dir = scratch("unchanged");
    std::fs::create_dir_all(dir.join("mesh")).unwrap();
    let input = "** units: SI-mm\n*INCLUDE, INPUT=mesh/part.inp\n*EQUATION\n2\n*STEP\n*FREQUENCY\n3\n*END STEP\n";
    let part = "*NODE\n1, 0, 0, 0\n*INCLUDE, INPUT=mesh/more.inp\n";
    std::fs::write(dir.join("job.inp"), input).unwrap();
    std::fs::write(dir.join("mesh/part.inp"), part).unwrap();
    std::fs::write(dir.join("mesh/more.inp"), "*NSET, NSET=A\n1\n").unwrap();
    let ccx = fake_ccx(&dir, "exit 0");
    let _ = run(&dir.join("job.inp"), &dir.join("run"), &ccx);
    let seen = |file: &str| std::fs::read_to_string(dir.join("run/ccx").join(file)).unwrap();
    assert_eq!(seen("job.inp"), input);
    assert_eq!(seen("mesh/part.inp"), part);
    assert_eq!(seen("mesh/more.inp"), "*NSET, NSET=A\n1\n");
}

#[test]
fn an_include_path_outside_the_input_folder_is_an_error() {
    let dir = scratch("escape");
    std::fs::write(dir.join("job.inp"), "*INCLUDE, INPUT=../other.inp\n").unwrap();
    let ccx = fake_ccx(&dir, "touch started");
    let err = run(&dir.join("job.inp"), &dir.join("run"), &ccx).unwrap_err();
    assert!(err.contains("no '..'"), "{err}");
    assert!(!dir.join("run/ccx/started").exists());
}

#[test]
fn a_failed_ccx_run_is_an_error_with_the_ccx_message() {
    let dir = scratch("failed");
    let ccx = fake_ccx(&dir, "echo ' *ERROR in readinput: bad card'\nexit 201");
    let err = run(&tet(&dir, ""), &dir.join("run"), &ccx).unwrap_err();
    assert!(err.contains("*ERROR in readinput: bad card"), "{err}");
    assert!(!dir.join("run/results.pvd").exists());
}

#[test]
fn an_invalid_result_file_is_an_error() {
    let dir = scratch("invalid");
    for (i, (body, why)) in [
        ("echo garbage > job.frd", "no end line"),
        (
            "printf '    2C\\n -1  xx\\n 9999\\n' > job.frd",
            "line 2: bad node line",
        ),
        ("true", "job.frd"),
    ]
    .into_iter()
    .enumerate()
    {
        let ccx = fake_ccx(&dir, body);
        let err = run(&tet(&dir, ""), &dir.join(format!("run-{i}")), &ccx).unwrap_err();
        assert!(err.contains(why), "{body}: {err}");
    }
}

#[test]
fn cancel_stops_ccx() {
    let dir = scratch("cancel");
    let exe = fake_ccx(&dir, "sleep 30");
    let token = CancelToken::new();
    let stop = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stop.cancel();
    });
    let clock = Instant::now();
    let e = ccx(&exe)
        .run_with(&tet(&dir, ""), &dir.join("run"), &token, &mut |_| {})
        .unwrap_err();
    assert!(e.is_cancelled(), "{e}");
    assert!(clock.elapsed() < Duration::from_secs(10));
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn the_time_limit_kills_the_whole_process_group() {
    let dir = scratch("tree");
    // The child of the fake ccx is in its group. Its pid goes to a file.
    let exe = fake_ccx(
        &dir,
        &format!("sleep 30 &\necho $! > {}/child.pid\nwait", dir.display()),
    );
    let mut s = ccx(&exe);
    // The script must write the pid file before the time limit.
    s.timeout = Some(Duration::from_secs(2));
    let e = s.run(&tet(&dir, ""), &dir.join("run")).unwrap_err();
    assert_eq!(e.code, code::TIME_LIMIT);
    assert!(e.message.contains("time limit of 2 s"), "{e}");
    assert!(e.message.contains("coarser mesh"), "{e}");
    let pid = std::fs::read_to_string(dir.join("child.pid")).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(!alive(pid.trim()), "child {pid} is still alive");
}

#[test]
fn the_memory_limit_kills_ccx() {
    // awk builds a string of about 134 MB and then loops.
    let dir = scratch("memory");
    let exe = fake_ccx(
        &dir,
        r#"exec awk 'BEGIN { s = "x"; for (i = 0; i < 27; i++) s = s s; while (1) { } }'"#,
    );
    let mut s = ccx(&exe);
    s.memory_limit = 50 << 20;
    let e = s.run(&tet(&dir, ""), &dir.join("run")).unwrap_err();
    assert_eq!(e.code, code::MEMORY);
    assert!(e.message.contains("memory limit of 50 MiB"), "{e}");
    assert!(e.message.contains("coarser mesh"), "{e}");
}

#[test]
fn a_new_ccx_has_no_time_limit_and_85_percent_of_the_ram() {
    let s = Ccx::new();
    assert_eq!(s.timeout, None);
    match fh_solver_ccx::total_memory() {
        Some(ram) => assert!(s.memory_limit > ram / 2 && s.memory_limit < ram),
        None => assert_eq!(s.memory_limit, u64::MAX),
    }
}

#[test]
fn a_run_folder_with_foreign_files_is_not_cleared() {
    let dir = scratch("foreign");
    let run_dir = dir.join("run");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("precious.txt"), "keep").unwrap();
    let e = run(&tet(&dir, ""), &run_dir, &dir.join("no-ccx")).unwrap_err();
    assert!(e.contains("not a run folder"), "{e}");
    assert!(run_dir.join("precious.txt").is_file());
    // An input inside a run folder is refused, because a run clears the folder.
    let inside = dir.join("inside");
    let _ = run(&tet(&dir, ""), &inside, &dir.join("no-ccx"));
    std::fs::write(inside.join("tet.inp"), TET).unwrap();
    let e = run(&inside.join("tet.inp"), &inside, &dir.join("no-ccx")).unwrap_err();
    assert!(e.contains("inside the run folder"), "{e}");
    assert!(inside.join("tet.inp").is_file());
}
