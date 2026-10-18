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

/// Start the adapter. The stdin pipe can send a `cancel` line.
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

/// A `.dat` file with the stress of two integration points of element 1. `ccx` prints
/// `sxy, sxz, syz`.
const DAT: &str = "
 stresses (elem, integ.pnt.,sxx,syy,szz,sxy,sxz,syz) for set FH_ALL and time  0.1000000E+01

         1   1  2.000000E+02  0.000000E+00  0.000000E+00  1.000000E+01  2.000000E+01  3.000000E+01
         1   2  1.000000E+02  0.000000E+00  0.000000E+00  1.000000E+01  2.000000E+01  3.000000E+01
";

fn point(grid: &Grid, field: &str, point: usize) -> Vec<f64> {
    let f = grid.point_field(field).unwrap();
    f.as_f64().unwrap()[point * f.components..][..f.components].to_vec()
}

#[test]
fn the_frd_results_become_vtu_pvd_and_a_summary() {
    let dir = scratch("frd");
    std::fs::write(dir.join("frd"), FRD).unwrap();
    std::fs::write(dir.join("dat"), DAT).unwrap();
    let ccx = fake_ccx(
        &dir,
        &format!(
            "echo 'CalculiX Version 9.9, Copyright'\ncp {0}/frd job.frd\ncp {0}/dat job.dat",
            dir.display()
        ),
    );
    let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &ccx);
    assert_eq!(code, Some(0), "{err}");
    let input = std::fs::read_to_string(dir.join("run/ccx/job.inp")).unwrap();
    for line in [
        "*SOLID SECTION, ELSET=FH_ALL, MATERIAL=STEEL",
        "2, 1, 1, 1.00000000000000e-3",
        "*NODE FILE\nU, RF\n*EL PRINT, ELSET=FH_ALL\nS\n*END STEP",
    ] {
        assert!(input.contains(line), "{line} not in\n{input}");
    }
    let pvd = std::fs::read_to_string(dir.join("run/results.pvd")).unwrap();
    assert!(pvd.contains("step-1.vtu") && pvd.contains("SI-mm"), "{pvd}");
    let grid = read_vtu(&std::fs::read(dir.join("run/step-1.vtu")).unwrap()).unwrap();
    assert_eq!(point(&grid, "U", 1), [1e-3, -2e-4, 0.0]);
    assert_eq!(point(&grid, "U", 0), [0.0; 3]);
    assert_eq!(point(&grid, "RF", 0), [-210.0, 0.0, 0.0]);
    // The points of a cube have the same weight times Jacobian, so S is the mean of the two
    // points, in the order xx, yy, zz, xy, yz, xz.
    let s = grid.cell_field("S").unwrap().as_f64().unwrap();
    assert_eq!(s, [150.0, 0.0, 0.0, 10.0, 30.0, 20.0]);
    let mises = grid.cell_field("MISES").unwrap().as_f64().unwrap();
    assert_eq!(mises, [26700f64.sqrt()]);
    let summary = std::fs::read_to_string(dir.join("run/summary.toon")).unwrap();
    assert!(summary.starts_with("schema: fh.run"), "{summary}");
    assert!(
        summary.contains("solver: \"ccx 9.9, unvalidated\""),
        "{summary}"
    );
    assert!(summary.contains("PULL,converged,1,2"), "{summary}");
}

/// In a distorted element the points have different weights times Jacobian, so `S` is the
/// volume mean of the points, and not the plain mean.
#[test]
fn the_element_stress_is_the_volume_mean_of_the_points() {
    let dir = scratch("volume-mean");
    // Nodes 6 and 7 move to z = 3: the Jacobian determinant is (2 + xi) / 8.
    let input = dir.join("wedge.inp");
    let wedge = CUBE
        .replace("6, 1, 0, 1", "6, 1, 0, 3")
        .replace("7, 1, 1, 1", "7, 1, 1, 3");
    std::fs::write(&input, wedge).unwrap();
    // sxx is 1 at the 4 points at xi = +g, and 0 at the 4 points at xi = -g.
    let mut dat = String::from(" stresses (elem, integ.pnt.,sxx,syy,szz,sxy,sxz,syz)\n");
    for p in 1..=8 {
        dat.push_str(&format!("1 {p} {} 0 0 0 0 0\n", (p + 1) % 2));
    }
    std::fs::write(dir.join("frd"), FRD).unwrap();
    std::fs::write(dir.join("dat"), dat).unwrap();
    let ccx = fake_ccx(
        &dir,
        &format!("cp {0}/frd job.frd\ncp {0}/dat job.dat", dir.display()),
    );
    let (code, err) = run(&input, &dir.join("run"), &ccx);
    assert_eq!(code, Some(0), "{err}");
    let grid = read_vtu(&std::fs::read(dir.join("run/step-1.vtu")).unwrap()).unwrap();
    let s = grid.cell_field("S").unwrap().as_f64().unwrap();
    // The plain mean is 0.5.
    let g = 1.0 / 3f64.sqrt();
    assert!((s[0] - (2.0 + g) / 4.0).abs() < 1e-12, "{s:?}");
}

#[test]
fn no_ccx_is_a_clear_error() {
    let dir = scratch("none");
    let (code, err) = run(&cube(&dir, ""), &dir.join("run"), &dir.join("no-ccx"));
    assert_eq!(code, Some(1));
    assert!(
        err.contains("cannot start") && err.contains("FH_CCX") && err.contains("docs/ccx.md"),
        "{err}"
    );
}

#[test]
fn the_thermal_keywords_go_to_ccx() {
    let dir = scratch("thermal");
    let input = dir.join("thermal.inp");
    let text = CUBE
        .replace(
            "*STEP",
            "*NSET, NSET=ALL\n1, 2, 3, 4, 5, 6, 7, 8\n\
             *INITIAL CONDITIONS, TYPE=TEMPERATURE\nALL, 20\n*STEP",
        )
        .replace("0.3\n", "0.3\n*EXPANSION\n1.2e-5\n")
        .replace("*END STEP", "*TEMPERATURE\nALL, 120\n8, 100\n*END STEP");
    std::fs::write(&input, text).unwrap();
    run(&input, &dir.join("run"), &dir.join("no-ccx"));
    let job = std::fs::read_to_string(dir.join("run/ccx/job.inp")).unwrap();
    for part in [
        "*ELASTIC\n2.10000000000000e5, 3.00000000000000e-1\n*EXPANSION\n1.20000000000000e-5\n",
        "*INITIAL CONDITIONS, TYPE=TEMPERATURE\nALL, 2.00000000000000e1\n*STEP\n",
        "*TEMPERATURE\nALL, 1.20000000000000e2\n8, 1.00000000000000e2\n*NODE FILE",
    ] {
        assert!(job.contains(part), "{part} is not in {job}");
    }
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

/// Each `ccx` failure tells why and what to do.
#[test]
fn a_ccx_failure_tells_why_and_what_to_do() {
    let dir = scratch("why");
    for (i, (body, parts)) in [
        (
            "echo ' *ERROR in e_c3d: nonpositive jacobian'\n\
             echo '        determinant in element           1'\necho\nexit 201",
            &[
                "*ERROR in e_c3d: nonpositive jacobian determinant in element 1",
                "job.log",
            ][..],
        ),
        (
            "kill -9 $$",
            &[
                "ccx stopped on signal 9",
                "memory",
                "coarser mesh",
                "job.log",
            ][..],
        ),
        (
            "echo 'line one'\necho 'the last line'\nexit 3",
            &["exit status: 3", "the last line", "job.log"][..],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let ccx = fake_ccx(&dir, body);
        let (code, err) = run(&cube(&dir, ""), &dir.join(format!("run-{i}")), &ccx);
        assert_eq!(code, Some(1), "{body}");
        for part in parts {
            assert!(err.contains(part), "{body}: {part} is not in {err}");
        }
    }
}

#[test]
fn an_invalid_result_file_is_an_error() {
    let dir = scratch("invalid");
    for (i, (body, why)) in [
        ("echo garbage > job.frd", "no end line"),
        (
            "printf '    1PSTEP 1 1 1\\n -4  DISP\\n -1  xx\\n 9999\\n' > job.frd",
            "line 3: bad value line",
        ),
        (
            "echo ' 9999' > job.frd; echo ' stresses (elem' > job.dat",
            "no displacement of step 1",
        ),
        (
            "printf '    1PSTEP 1 1 1\\n -4  DISP\\n -1         2 1.00000E-03\\n 9999\\n' > job.frd
             echo ' stresses (elem' > job.dat",
            "no stress of element 1 in step 1",
        ),
        ("echo ' 9999' > job.frd", "job.dat"),
        ("true", "job.frd"),
    ]
    .into_iter()
    .enumerate()
    {
        let ccx = fake_ccx(&dir, body);
        let (code, err) = run(&cube(&dir, ""), &dir.join(format!("run-{i}")), &ccx);
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

/// `ccx` reads at most 20 characters of a number. A longer number loses the end of its
/// exponent: -1.33e-17 becomes -0.133. Thus each number of `job.inp` must fit.
#[test]
fn each_number_of_the_ccx_input_fits_in_20_characters() {
    let dir = scratch("width");
    let input = dir.join("width.inp");
    let text = CUBE.replace("4, 0, 1, 0\n", "4, -1.3302787975829e-17, 1, -4.7e-117\n");
    assert_ne!(text, CUBE);
    std::fs::write(&input, text).unwrap();
    run(&input, &dir.join("run"), &dir.join("no-ccx"));
    let job = std::fs::read_to_string(dir.join("run/ccx/job.inp")).unwrap();
    let line = job.lines().find(|l| l.starts_with("4, ")).unwrap();
    let values: Vec<f64> = line
        .split(", ")
        .skip(1)
        .map(|v| {
            assert!(v.len() <= 20, "{v} has {} characters", v.len());
            v.parse().unwrap()
        })
        .collect();
    assert!(
        (values[0] / -1.3302787975829e-17 - 1.0).abs() < 1e-12,
        "{line}"
    );
    assert!((values[2] / -4.7e-117 - 1.0).abs() < 1e-12, "{line}");
}

/// `ccx` solves a model with a rigid body motion with no error, and its result is wrong
/// (displacements of 1e12 mm). Thus the adapter refuses the model before `ccx` starts.
#[test]
fn a_model_that_can_move_as_a_rigid_body_is_a_clear_error() {
    let dir = scratch("rigid");
    let ccx = fake_ccx(&dir, "touch started");
    let held = "*BOUNDARY\nFIXED, 1, 3\n2, 1, 1, 0.001\n";
    for (i, (support, mode)) in [
        ("*CLOAD\n2, 1, 100.0\n", "translation in x"),
        ("*BOUNDARY\nFIXED, 1, 1\n", "translation in y"),
        ("*BOUNDARY\n1, 1, 3\n5, 1, 3\n", "rotation about z"),
    ]
    .into_iter()
    .enumerate()
    {
        let input = dir.join(format!("rigid-{i}.inp"));
        std::fs::write(&input, CUBE.replace(held, support)).unwrap();
        let (code, err) = run(&input, &dir.join(format!("run-{i}")), &ccx);
        assert_eq!(code, Some(1), "{support}");
        assert!(
            err.contains("can move as a rigid body") && err.contains(mode),
            "{support}: {err}"
        );
        assert!(err.contains("*BOUNDARY"), "{err}");
        assert!(!dir.join(format!("run-{i}/ccx/started")).exists());
    }
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

/// The real `ccx` from `FH_CCX` or PATH, or `None` with a message when it is absent. With
/// `FH_REQUIRE_CCX=1`, as in CI, an absent `ccx` fails the test.
fn real_ccx() -> Option<PathBuf> {
    let exe = std::env::var_os("FH_CCX").map_or_else(|| PathBuf::from("ccx"), PathBuf::from);
    if Command::new(&exe).arg("-v").output().is_ok() {
        return Some(exe);
    }
    assert!(
        std::env::var_os("FH_REQUIRE_CCX").is_none_or(|v| v != "1"),
        "FH_REQUIRE_CCX=1, but ccx does not start"
    );
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
    // The integration point stress of `job.dat` has 7 significant digits.
    let s = bar.cell_field("S").unwrap().as_f64().unwrap();
    let s = mean(&s.iter().step_by(6).copied().collect::<Vec<_>>());
    assert!((s / 100.0 - 1.0).abs() < 1e-6, "bar s_xx {s}");
}
