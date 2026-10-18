//! `fh-solve-ccx` with small fake `ccx` scripts, and with the real `ccx` when it is installed.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fh_io::vtu::{read_vtu, Grid};

const EXE: &str = env!("CARGO_BIN_EXE_fh-solve-ccx");

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

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

/// Start the adapter. The stdin pipe stays open, because its end means cancel.
fn start(input: &Path, run: &Path, ccx: &Path) -> Child {
    Command::new(EXE)
        .arg(input)
        .arg(run)
        .env("FH_CCX", ccx)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Run the adapter to the end: the exit code and stderr.
fn run(input: &Path, run: &Path, ccx: &Path) -> (Option<i32>, String) {
    let mut child = start(input, run, ccx);
    let _stdin = child.stdin.take();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().next(), Some("fh-solver-protocol 2"));
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

/// One C3D8 cube, tied at x = 0 and pulled at x = 1.
const CUBE: &str = "** units: SI-mm
*NODE
1, 0, 0, 0
2, 1, 0, 0
3, 1, 1, 0
4, 0, 1, 0
5, 0, 0, 1
6, 1, 0, 1
7, 1, 1, 1
8, 0, 1, 1
*ELEMENT, TYPE=C3D8
1, 1, 2, 3, 4, 5, 6, 7, 8
*NSET, NSET=FIXED
1, 4, 5, 8
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

fn cube(dir: &Path, extra: &str) -> PathBuf {
    let file = dir.join("cube.inp");
    std::fs::write(&file, CUBE.replace("*STEP", &format!("{extra}*STEP"))).unwrap();
    file
}

/// An `.frd` file with two increments of step 1. The second increment replaces the first.
const FRD: &str = "    1C
    1PSTEP                         1           1           1
  100CL  101 0.500000000           8                     0    1           1
 -4  DISP        4    1
 -1         2 5.00000E-04 0.00000E+00 0.00000E+00
 -3
    1PSTEP                         2           2           1
  100CL  101 1.000000000           8                     0    1           1
 -4  DISP        4    1
 -5  D1          1    2    1    0
 -1         2 1.00000E-03-2.00000E-04 0.00000E+00
 -3
 -4  FORC        4    1
 -1         1-2.10000E+02 0.00000E+00 0.00000E+00
 -3
 -4  STRESS      6    1
 -1         1 2.00000E+02 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00
 -1         2 1.00000E+02 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00
 -3
 -4  ERROR       1    1
 -1         1 1.00000E+00
 -3
 9999
";

fn point(grid: &Grid, field: &str, point: usize) -> Vec<f64> {
    let f = grid.point_field(field).unwrap();
    f.as_f64().unwrap()[point * f.components..][..f.components].to_vec()
}

#[test]
fn the_frd_results_become_vtu_pvd_and_a_summary() {
    let dir = scratch("frd");
    let frd = dir.join("frd");
    std::fs::write(&frd, FRD).unwrap();
    let ccx = fake_ccx(
        &dir,
        &format!(
            "echo 'CalculiX Version 9.9, Copyright'\ncp {} job.frd",
            frd.display()
        ),
    );
    let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &ccx);
    assert_eq!(code, Some(0), "{err}");
    let input = std::fs::read_to_string(dir.join("run/ccx/job.inp")).unwrap();
    for line in [
        "*SOLID SECTION, ELSET=FH_ALL, MATERIAL=STEEL",
        "2, 1, 1, 1.00000000000000e-3",
        "*NODE FILE\nU, RF\n*EL FILE\nS\n*END STEP",
    ] {
        assert!(input.contains(line), "{line} not in\n{input}");
    }
    let pvd = std::fs::read_to_string(dir.join("run/results.pvd")).unwrap();
    assert!(pvd.contains("step-1.vtu") && pvd.contains("SI-mm"), "{pvd}");
    let grid = read_vtu(&std::fs::read(dir.join("run/step-1.vtu")).unwrap()).unwrap();
    assert_eq!(point(&grid, "U", 1), [1e-3, -2e-4, 0.0]);
    assert_eq!(point(&grid, "U", 0), [0.0; 3]);
    assert_eq!(point(&grid, "RF", 0), [-210.0, 0.0, 0.0]);
    let s = grid.cell_field("S").unwrap().as_f64().unwrap();
    // The mean of the 8 nodes: 200 and 100 on two nodes, 0 on the others.
    assert_eq!(s[0], 300.0 / 8.0);
    assert_eq!(grid.cell_field("MISES").unwrap().as_f64().unwrap(), [37.5]);
    let summary = std::fs::read_to_string(dir.join("run/summary.toon")).unwrap();
    assert!(summary.starts_with("schema: fh.run"), "{summary}");
    assert!(
        summary.contains("solver: \"ccx 9.9, unvalidated\""),
        "{summary}"
    );
    assert!(summary.contains("PULL,converged,1,2"), "{summary}");
}

#[test]
fn no_ccx_is_a_clear_error() {
    let dir = scratch("none");
    let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &dir.join("no-ccx"));
    assert_eq!(code, Some(1));
    assert!(
        err.contains("cannot start") && err.contains("FH_CCX"),
        "{err}"
    );
}

#[test]
fn a_failed_ccx_run_is_an_error_with_the_ccx_message() {
    let dir = scratch("failed");
    let ccx = fake_ccx(&dir, "echo ' *ERROR in readinput: bad card'\nexit 201");
    let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &ccx);
    assert_eq!(code, Some(1));
    assert!(err.contains("*ERROR in readinput: bad card"), "{err}");
    assert!(!dir.join("run/results.pvd").exists());
}

#[test]
fn an_invalid_frd_file_is_an_error() {
    let dir = scratch("invalid");
    for (body, why) in [
        ("echo garbage > job.frd", "no end line"),
        (
            "printf '    1PSTEP 1 1 1\\n -4  DISP\\n -1  xx\\n 9999\\n' > job.frd",
            "line 3: bad value line",
        ),
        ("printf ' 9999\\n' > job.frd", "no displacement of step 1"),
        ("true", "job.frd"),
    ] {
        let ccx = fake_ccx(&dir, body);
        let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &ccx);
        assert_eq!(code, Some(1), "{body}");
        assert!(err.contains(why), "{body}: {err}");
    }
}

#[test]
fn a_keyword_outside_the_subset_is_a_clear_error() {
    let dir = scratch("keyword");
    let input = cube(&dir, "*EQUATION\n2\n2, 2, 1.0, 3, 2, -1.0\n");
    let (code, err) = run(&input, &dir.join("run"), &dir.join("no-ccx"));
    assert_eq!(code, Some(1));
    assert!(
        err.contains("*EQUATION is not in the keyword subset of the ccx adapter"),
        "{err}"
    );
}

#[test]
fn cancel_stops_ccx() {
    let dir = scratch("cancel");
    let ccx = fake_ccx(&dir, "sleep 30");
    let mut child = start(&cube(&dir, ""), &dir.join("run"), &ccx);
    let mut header = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut header)
        .unwrap();
    assert_eq!(header.trim(), "fh-solver-protocol 2");
    let clock = Instant::now();
    child.stdin.take().unwrap().write_all(b"cancel\n").unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130));
    assert!(clock.elapsed() < Duration::from_secs(10));
}

/// The real `ccx` from `FH_CCX` or PATH, or `None` with a message when it is absent.
fn real_ccx() -> Option<PathBuf> {
    let exe = std::env::var_os("FH_CCX").map_or_else(|| PathBuf::from("ccx"), PathBuf::from);
    if Command::new(&exe).arg("-v").output().is_ok() {
        return Some(exe);
    }
    eprintln!("skipped: ccx not found. Set FH_CCX or put ccx on PATH to run this test");
    None
}

/// One component of U or RF on each node of a node set of a result grid.
fn on_set(grid: &Grid, field: &str, set: &str, component: usize) -> Vec<f64> {
    let name = format!("NSET_{set}");
    let flags = grid
        .point_data
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(&name));
    let Some(fh_io::vtu::Values::I32(flags)) = flags.map(|f| &f.values) else {
        panic!("no integer array {name}")
    };
    (0..flags.len())
        .filter(|i| flags[*i] != 0)
        .map(|i| point(grid, field, i)[component])
        .collect()
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len() as f64
}

/// The cantilever and the bar-pressure cases through the real `ccx`. The limits are for
/// `ccx` 2.23, whose `.frd` file has 6 significant digits.
#[test]
fn real_ccx_matches_the_cantilever_and_bar_pressure_references() {
    let Some(ccx) = real_ccx() else { return };
    let dir = scratch("real");
    let solve = |case: &str| {
        let run_dir = dir.join(case.replace('/', "-"));
        let (code, err) = run(&repo(&format!("validation/cases/{case}")), &run_dir, &ccx);
        assert_eq!(code, Some(0), "{case}: {err}");
        read_vtu(&std::fs::read(run_dir.join("step-1.vtu")).unwrap()).unwrap()
    };
    // Reference -0.01919619048 mm (case.toml). The C3D8I value of mesh-32 is 0.72 % soft.
    let tip = mean(&on_set(&solve("cantilever/mesh-32.inp"), "U", "tip", 2));
    assert!(
        (tip / -0.01919619048 - 1.0).abs() < 0.01,
        "cantilever tip {tip}"
    );
    let bar = solve("bar-pressure/bar.inp");
    let u = mean(&on_set(&bar, "U", "LOAD", 0));
    assert!((u / 0.047619047619047616 - 1.0).abs() < 1e-5, "bar u {u}");
    let rf: f64 = on_set(&bar, "RF", "SUPPORT", 0).iter().sum();
    assert!((rf / -20000.0 - 1.0).abs() < 1e-5, "bar rf {rf}");
    // `ccx` extrapolates the stress to the nodes with an error of about 1e-5.
    let s = bar.cell_field("S").unwrap().as_f64().unwrap();
    let s = mean(&s.iter().step_by(6).copied().collect::<Vec<_>>());
    assert!((s / 100.0 - 1.0).abs() < 1e-4, "bar s_xx {s}");
}
