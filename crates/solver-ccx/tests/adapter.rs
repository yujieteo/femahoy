//! The `ccx` run with small fake `ccx` scripts, and with the real `ccx` when it is installed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use fh_io::vtu::{read_vtu, Grid};
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

/// An `.frd` file with a tet of 4 nodes. Step 1 has two increments, and the second replaces
/// the first. Step 2 has a displacement only.
const FRD: &str = "    1C
    2C                             4                                     1
 -1         1 0.00000E+00 0.00000E+00 0.00000E+00
 -1         2 1.00000E+00 0.00000E+00 0.00000E+00
 -1         3 0.00000E+00 1.00000E+00 0.00000E+00
 -1         4 0.00000E+00 0.00000E+00 1.00000E+00
 -3
    3C                             1                                     1
 -1         1    3    0    1
 -2         1         2         3         4
 -3
    1PSTEP                         1           1           1
  100CL  101 5.00000E-01           4                     0    1           1
 -4  DISP        4    1
 -1         2 5.00000E-04 0.00000E+00 0.00000E+00
 -3
    1PSTEP                         2           2           1
  100CL  102 1.00000E+00           4                     0    2           1
 -4  DISP        4    1
 -5  D1          1    2    1    0
 -1         2 1.00000E-03-2.00000E-04 0.00000E+00
 -3
 -4  FORC        4    1
 -1         1-2.10000E+02 0.00000E+00 0.00000E+00
 -3
 -4  STRESS      6    1
 -1         1 2.00000E+02 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00
 -1         2 1.00000E+02 0.00000E+00 0.00000E+00 3.00000E+01 0.00000E+00 0.00000E+00
 -3
 -4  ERROR       1    1
 -1         1 1.00000E+00
 -3
    1PSTEP                         3           1           2
  100CL  103 2.00000E+00 4 0 0 0
 -4  DISP        4    1
 -1         2 2.00000E-03 0.00000E+00 0.00000E+00
 -3
 9999
";

fn point(grid: &Grid, field: &str, point: usize) -> Vec<f64> {
    let f = grid.point_field(field).unwrap();
    f.as_f64().unwrap()[point * f.components..][..f.components].to_vec()
}

fn read_grid(path: PathBuf) -> Grid {
    read_vtu(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn every_frd_field_becomes_vtu_and_pvd() {
    let dir = scratch("frd");
    std::fs::write(dir.join("frd"), FRD).unwrap();
    let ccx = fake_ccx(
        &dir,
        &format!(
            "echo 'CalculiX Version 9.9, Copyright'\ncp {0}/frd job.frd",
            dir.display()
        ),
    );
    if let Err(err) = run(&tet(&dir, ""), &dir.join("run"), &ccx) {
        panic!("{err}");
    }
    let pvd = std::fs::read_to_string(dir.join("run/results.pvd")).unwrap();
    assert!(pvd.contains("SI-mm"), "{pvd}");
    assert!(
        pvd.contains("timestep=\"1\"") && pvd.contains("timestep=\"2\""),
        "{pvd}"
    );
    let grid = read_grid(dir.join("run/step-1.vtu"));
    assert_eq!(point(&grid, "U", 1), [1e-3, -2e-4, 0.0]);
    assert_eq!(point(&grid, "U", 0), [0.0; 3]);
    assert_eq!(point(&grid, "RF", 0), [-210.0, 0.0, 0.0]);
    assert_eq!(point(&grid, "S", 1), [100.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
    assert_eq!(point(&grid, "MISES", 0), [200.0]);
    assert_eq!(point(&grid, "ERROR", 0), [1.0]);
    let two = read_grid(dir.join("run/step-2.vtu"));
    assert_eq!(point(&two, "U", 1), [2e-3, 0.0, 0.0]);
    assert!(two.point_field("RF").is_none());
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
fn no_ccx_is_a_clear_error() {
    let dir = scratch("none");
    let err = run(&tet(&dir, ""), &dir.join("run"), &dir.join("no-ccx")).unwrap_err();
    assert!(
        err.contains("cannot start") && err.contains("FH_CCX") && err.contains("README.md"),
        "{err}"
    );
}

#[test]
fn a_failed_ccx_run_is_an_error_with_the_ccx_message() {
    let dir = scratch("failed");
    let ccx = fake_ccx(&dir, "echo ' *ERROR in readinput: bad card'\nexit 201");
    let err = run(&tet(&dir, ""), &dir.join("run"), &ccx).unwrap_err();
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
        let err = run(&tet(&dir, ""), &dir.join(format!("run-{i}")), &ccx).unwrap_err();
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

/// A test input of this crate.
fn input(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/inputs")
        .join(name)
}

/// Solve a test input with the real `ccx`, and read the VTU file of each step.
fn solve(ccx: &Path, name: &str) -> (PathBuf, Vec<Grid>) {
    let run_dir = scratch(name);
    if let Err(err) = run(&input(&format!("{name}.inp")), &run_dir, ccx) {
        panic!("{name}: {err}");
    }
    let pvd = std::fs::read_to_string(run_dir.join("results.pvd")).unwrap();
    let grids = fh_io::vtu::read_pvd(&pvd)
        .unwrap()
        .iter()
        .map(|e| read_grid(run_dir.join(&e.file)))
        .collect();
    (run_dir, grids)
}

/// The corners of each cell in VTK order are the first nodes. Each midside node must be at the
/// middle of the edge that VTK gives it, so a wrong node order shows here.
fn assert_midside_nodes_are_midpoints(grid: &Grid) {
    // Cell type, number of corners, and the corner pairs of the midside nodes in VTK order.
    type Edges<'a> = (u8, usize, &'a [(usize, usize)]);
    let edges: [Edges; 3] = [
        (24, 4, &[(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)]),
        (
            25,
            8,
            &[
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ],
        ),
        (
            26,
            6,
            &[
                (0, 1),
                (1, 2),
                (2, 0),
                (3, 4),
                (4, 5),
                (5, 3),
                (0, 3),
                (1, 4),
                (2, 5),
            ],
        ),
    ];
    for (cell, kind) in grid.cells.iter().zip(&grid.cell_types) {
        let (_, corners, pairs) = edges.iter().find(|e| e.0 == *kind).unwrap();
        let x = |i: usize| grid.points[cell[i] as usize];
        for (k, (a, b)) in pairs.iter().enumerate() {
            let middle: Vec<f64> = (0..3).map(|c| (x(*a)[c] + x(*b)[c]) / 2.0).collect();
            let node = x(corners + k);
            assert!(
                (0..3).all(|c| (node[c] - middle[c]).abs() < 1e-9),
                "cell type {kind}: node {} is not at the middle of the edge {a}-{b}",
                corners + k
            );
        }
    }
}

fn largest(grid: &Grid, field: &str) -> f64 {
    let values = grid.point_field(field).unwrap().as_f64().unwrap();
    values.iter().fold(0.0, |m: f64, v| m.max(v.abs()))
}

/// `ccx` writes the nodes of a 20-node and of a 15-node element in its own order. The cells
/// of the VTU file must follow the order of VTK.
#[test]
fn real_ccx_hex20_and_wedge15_elements_keep_their_shape() {
    let Some(ccx) = real_ccx() else { return };
    for (name, kind) in [("hex20", 25), ("wedge15", 26)] {
        let (_, grids) = solve(&ccx, name);
        assert_eq!(grids[0].cell_types, [kind], "{name}");
        assert_midside_nodes_are_midpoints(&grids[0]);
        assert!(largest(&grids[0], "U") > 0.0, "{name}");
    }
}

/// A shell element is a solid element in the `.frd` file. The `.dat` file stays for the chat.
#[test]
fn real_ccx_shell_results_are_in_the_vtu_file_and_the_dat_file() {
    let Some(ccx) = real_ccx() else { return };
    let (run_dir, grids) = solve(&ccx, "shell-s8r");
    assert_eq!(grids.len(), 1);
    assert_eq!(grids[0].cell_types, [25]);
    assert_midside_nodes_are_midpoints(&grids[0]);
    for field in ["U", "S", "MISES", "E"] {
        assert!(largest(&grids[0], field) > 0.0, "{field}");
    }
    let dat = std::fs::read_to_string(run_dir.join("ccx/job.dat")).unwrap();
    assert!(
        dat.contains("displacements (vx,vy,vz) for set TOP"),
        "{dat}"
    );
}

#[test]
fn real_ccx_gives_one_grid_for_each_mode_of_a_frequency_step() {
    let Some(ccx) = real_ccx() else { return };
    let (run_dir, grids) = solve(&ccx, "shell-modal");
    assert_eq!(grids.len(), 3);
    assert!(grids.iter().all(|g| largest(g, "U") > 0.0));
    let pvd = std::fs::read_to_string(run_dir.join("results.pvd")).unwrap();
    let times: Vec<f64> = fh_io::vtu::read_pvd(&pvd)
        .unwrap()
        .iter()
        .map(|e| e.time)
        .collect();
    assert!(
        times[0] > 0.0 && times.windows(2).all(|w| w[0] < w[1]),
        "{times:?}"
    );
}

/// A plastic material gives the plastic strain, and the last increment of the step is the
/// grid of the step.
#[test]
fn real_ccx_plastic_strain_is_in_the_vtu_file() {
    let Some(ccx) = real_ccx() else { return };
    let (_, grids) = solve(&ccx, "plastic");
    assert_eq!(grids.len(), 1);
    for field in ["U", "S", "MISES", "E", "PEEQ"] {
        assert!(largest(&grids[0], field) > 0.0, "{field}");
    }
}

#[test]
fn progress_reports_the_step_and_increment_of_ccx() {
    let dir = scratch("progress");
    let exe = fake_ccx(
        &dir,
        "printf ' STEP 1\\n INCREMENT 1\\n ATTEMPT 1\\n INCREMENT 2\\n STEP 2\\n INCREMENT 1\\n'; sleep 30",
    );
    let token = CancelToken::new();
    let stop = token.clone();
    let mut seen = Vec::new();
    let e = ccx(&exe)
        .run_with(&tet(&dir, ""), &dir.join("run"), &token, &mut |p| {
            seen.push((p.step, p.increment));
            if seen.len() == 3 {
                stop.cancel();
            }
        })
        .unwrap_err();
    assert!(e.is_cancelled(), "{e}");
    assert_eq!(seen, [(1, 1), (1, 2), (2, 1)]);
}
