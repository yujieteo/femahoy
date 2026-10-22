//! `fh run`: one solve with the real ccx on the input of a validation case, and the stop of
//! ccx when `fh run` ends for any reason. The stop tests use a small fake `ccx` that waits. The
//! OpenFOAM tests use a fake `openfoam` that writes the files of each program, the GetDP tests a
//! fake `gmsh` and a fake `getdp`, the Chrono tests a fake Python, and the LAMMPS tests a fake
//! `lmp`.
mod common;
use common::{ccx_solver, p, root, scratch, script, stderr, stdout};

use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A fake ccx. It writes its pid and the pid of its child, then waits. It ignores SIGTERM, so
/// only a kill of its process group stops it.
const WAITING_CCX: &str = "#!/bin/sh\ntrap '' TERM\nsleep 300 &\n\
    echo $$ $! > PIDS.tmp\nmv PIDS.tmp PIDS\nwait\n";

struct Lab {
    dir: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = scratch(name);
        script(
            &dir.join("ccx"),
            &WAITING_CCX.replace("PIDS", p(&dir.join("pids"))),
        );
        Lab { dir }
    }

    /// `fh run MODEL ARGS` with the fake ccx. It ends with the stop of the test.
    fn start(&self, args: &[&str]) -> Child {
        let model = root().join("validation/cases/bar-pressure/job.inp");
        Command::new(env!("CARGO_BIN_EXE_fh"))
            .args(["run", p(&model)])
            .args(args)
            .env("FH_STATE", self.dir.join("state"))
            .env("FH_CCX", self.dir.join("ccx"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start fh run")
    }

    /// The pids of the fake ccx, when it runs.
    fn pids(&self) -> Vec<String> {
        wait_for("the fake ccx", || {
            let text = fs::read_to_string(self.dir.join("pids")).ok()?;
            Some(text.split_whitespace().map(str::to_owned).collect())
        })
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn wait_for<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < end, "timeout: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn signal(pid: u32, name: &str) {
    let status = Command::new("kill").arg(name).arg(pid.to_string()).status();
    assert!(status.unwrap().success());
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Each pid is gone: the fake ccx and its child.
#[track_caller]
fn assert_stopped(pids: &[String]) {
    assert_eq!(pids.len(), 2, "{pids:?}");
    wait_for(&format!("the stop of {pids:?}"), || {
        (!pids.iter().any(|p| alive(p))).then_some(())
    });
}

/// Send `name` to `fh run` after ccx started. `fh run` must end by itself and stop ccx.
fn stop_with(name: &str, name_of_test: &str) -> (std::process::Output, Vec<String>) {
    let lab = Lab::new(name_of_test);
    let child = lab.start(&[]);
    let pids = lab.pids();
    assert!(pids.iter().all(|p| alive(p)), "ccx runs before the signal");
    signal(child.id(), name);
    let out = child.wait_with_output().unwrap();
    assert_stopped(&pids);
    (out, pids)
}

#[test]
fn ctrl_c_stops_ccx() {
    let (out, _) = stop_with("-INT", "run-int");
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
}

#[test]
fn sigterm_stops_ccx() {
    let (out, _) = stop_with("-TERM", "run-term");
    assert_eq!(out.status.code(), Some(130), "{}", stderr(&out));
}

/// SIGKILL gives `fh run` no time. The guard process of the solver stops ccx.
#[test]
fn a_kill_of_fh_run_stops_ccx() {
    let (out, _) = stop_with("-KILL", "run-kill");
    assert_eq!(out.status.code(), None, "ended by the signal");
}

#[test]
fn the_time_limit_stops_ccx_and_fails_the_run() {
    let lab = Lab::new("run-timeout");
    let out = lab.start(&["--timeout", "3"]).wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(stderr(&out).contains("time limit"), "{}", stderr(&out));
    assert_stopped(&lab.pids());
}

/// The mean of the first values of `S` is the stress along the bar.
fn mean_of_first(values: &[f64], components: usize) -> f64 {
    let first: Vec<f64> = values.iter().step_by(components).copied().collect();
    first.iter().sum::<f64>() / first.len() as f64
}

/// The real ccx solves the input of `bar-pressure` with a request for the fields of the page.
/// The result files hold the known solution: 100 MPa along the bar and a tip of 0.047619 mm.
#[test]
fn fh_run_solves_a_validation_input_and_writes_the_result_files() {
    let Some(solver) = ccx_solver() else { return };
    let dir = scratch("run-ccx");
    let case = fs::read_to_string(root().join("validation/cases/bar-pressure/job.inp")).unwrap();
    let input = dir.join("job.inp");
    let text = case.replace("*END STEP", "*NODE FILE\nU\n*EL FILE\nS\n*END STEP");
    assert_ne!(text, case);
    fs::write(&input, &text).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(&input)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_CCX", &solver)
        .output()
        .unwrap();
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");

    let run = dir.join("state/runs/run-000001");
    let results = run.join("out");
    let paths = format!(
        "fh run: text results {0}/ccx/job.dat (print requests), {0}/ccx/job.sta (steps)",
        results.display()
    );
    assert_eq!(shown.lines().last(), Some(paths.as_str()), "{shown}");
    for file in ["ccx/job.dat", "ccx/job.sta", "results.pvd", "step-1.vtu"] {
        assert!(results.join(file).is_file(), "no {file}: {shown}");
    }
    assert_eq!(fs::read_to_string(run.join("in/input.inp")).unwrap(), text);
    assert_eq!(
        fs::read_to_string(run.join("source")).unwrap(),
        fs::canonicalize(&input).unwrap().display().to_string()
    );
    assert!(dir.join("state/previews/1/input.inp").is_file());
    let pvd = fs::read_to_string(results.join("results.pvd")).unwrap();
    assert!(pvd.contains("step-1.vtu") && pvd.contains("SI-mm"), "{pvd}");

    let grid = fh_core::vtu::read_vtu(&fs::read(results.join("step-1.vtu")).unwrap()).unwrap();
    let field = |name: &str| grid.point_field(name).unwrap().as_f64().unwrap().to_vec();
    let stress = mean_of_first(&field("S"), 6);
    assert!((stress - 100.0).abs() < 1e-3, "S xx {stress}");
    let tip = field("U").iter().step_by(3).fold(0.0_f64, |m, v| m.max(*v));
    let want = 100.0 / 210000.0 * 100.0;
    assert!((tip - want).abs() < 1e-5 * want, "tip {tip} {want}");
    let _ = fs::remove_dir_all(&dir);
}

/// A fake `openfoam`. It writes its arguments to CALLS, and the files that each program writes:
/// the mesh, a time folder, and a legacy VTK file with 1 hexahedron and its cell fields. With
/// FAIL as its program, it fails as OpenFOAM does.
const FAKE_OPENFOAM: &str = r#"#!/bin/sh
echo "$@" >> CALLS
case "$1" in
blockMesh) mkdir -p constant/polyMesh ;;
FAIL) printf '\n--> FOAM FATAL ERROR: (openfoam-2512)\nUnknown solver FAIL\n\n    From main\n' ; exit 1 ;;
simpleFoam) mkdir -p 100 ;;
foamToVTK) mkdir -p VTK && printf '# vtk DataFile Version 2.0\ncase\nASCII\nDATASET UNSTRUCTURED_GRID\nFIELD FieldData 1\nTimeValue 1 1 float\n100\nPOINTS 8 float\n0 0 0 1 0 0 1 1 0 0 1 0\n0 0 1 1 0 1 1 1 1 0 1 1\nCELLS 1 9\n8 0 1 2 3 4 5 6 7\nCELL_TYPES 1\n12\nCELL_DATA 1\nFIELD FieldData 2\np 1 1 float\n-0.5\nU 3 1 float\n1 0.25 0\n' > VTK/case_100.vtk ;;
esac
echo End
"#;

/// An OpenFOAM case with a mesh dictionary and the application `application`, and the fake
/// `openfoam`.
fn foam_lab(name: &str, application: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let case = dir.join("case");
    for folder in ["system", "constant", "0"] {
        fs::create_dir_all(case.join(folder)).unwrap();
    }
    fs::write(
        case.join("system/controlDict"),
        format!("application {application};\n"),
    )
    .unwrap();
    fs::write(case.join("system/blockMeshDict"), "blocks ();\n").unwrap();
    fs::write(case.join("0/U"), "internalField uniform (1 0 0);\n").unwrap();
    script(
        &dir.join("openfoam"),
        &FAKE_OPENFOAM.replace("CALLS", p(&dir.join("calls"))),
    );
    (dir, case)
}

fn fh_run_foam(dir: &std::path::Path, case: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(case)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_OPENFOAM", dir.join("openfoam"))
        .output()
        .unwrap()
}

/// `fh run CASE` solves a copy of the case with blockMesh, the application of the case and
/// foamToVTK, and writes the cell fields of the last time to the VTU file of the page.
#[test]
fn fh_run_solves_an_openfoam_case_with_each_program_in_turn() {
    let (dir, case) = foam_lab("run-foam", "simpleFoam");
    let out = fh_run_foam(&dir, &case);
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");
    assert!(
        shown.contains("  program blockMesh\n  program simpleFoam\n  program foamToVTK\n"),
        "{shown}"
    );

    let run = dir.join("state/runs/run-000001");
    let work = run.join("out/foam");
    let paths = format!(
        "fh run: text results {0}/log.<program> (output of each program), {0}/postProcessing \
         (function objects)",
        work.display()
    );
    assert_eq!(shown.lines().last(), Some(paths.as_str()), "{shown}");
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    let work = fs::canonicalize(&work).unwrap();
    let want = format!(
        "blockMesh -case {0}\nsimpleFoam -case {0}\nfoamToVTK -latestTime -legacy -ascii \
         -no-boundary -no-point-data -case {0}\n",
        work.display()
    );
    assert_eq!(calls, want);
    for file in ["log.blockMesh", "log.simpleFoam", "log.foamToVTK", "0/U"] {
        assert!(work.join(file).is_file(), "no {file}: {shown}");
    }
    assert_eq!(
        fs::read_to_string(work.join("log.simpleFoam")).unwrap(),
        "End\n"
    );
    let copy = run.join("in/system/controlDict");
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    assert!(
        !dir.join("state/previews").exists(),
        "a case has no preview"
    );

    let pvd = fs::read_to_string(run.join("out/results.pvd")).unwrap();
    assert!(
        pvd.contains("step-1.vtu") && pvd.contains("timestep=\"100\""),
        "{pvd}"
    );
    let grid = fh_core::vtu::read_vtu(&fs::read(run.join("out/step-1.vtu")).unwrap()).unwrap();
    assert_eq!((grid.points.len(), grid.cells.len()), (8, 1));
    assert!(
        grid.point_data.is_empty(),
        "a point field U moves the mesh in the viewer"
    );
    let u = grid.cell_field("U").unwrap();
    assert_eq!((u.components, u.as_f64()), (3, Some(&[1.0, 0.25, 0.0][..])));
    let _ = fs::remove_dir_all(&dir);
}

/// A program that fails ends the run with the fatal error of OpenFOAM and the path of its log.
#[test]
fn a_failed_openfoam_program_fails_the_run_with_its_fatal_error() {
    let (dir, case) = foam_lab("run-foam-fail", "FAIL");
    let out = fh_run_foam(&dir, &case);
    let errors = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{errors}");
    assert!(
        errors.contains(
            "FAIL failed (exit status: 1): FOAM FATAL ERROR: (openfoam-2512) Unknown solver FAIL. \
             Correct"
        ),
        "{errors}"
    );
    assert!(errors.contains("out/foam/log.FAIL"), "{errors}");
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    assert_eq!(
        calls.lines().count(),
        2,
        "no foamToVTK after the failure: {calls}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A case with `system/setFieldsDict` gets its initial fields from setFields after the mesh, for
/// example the water column of an interFoam case.
#[test]
fn fh_run_sets_the_initial_fields_with_set_fields() {
    let (dir, case) = foam_lab("run-foam-set-fields", "interFoam");
    fs::write(case.join("system/setFieldsDict"), "regions ();\n").unwrap();
    let out = fh_run_foam(&dir, &case);
    let shown = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "{shown}{}", stderr(&out));
    assert!(
        shown.contains("  program blockMesh\n  program setFields\n  program interFoam\n"),
        "{shown}"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// A fake `gmsh` and a fake `getdp`. They write their arguments to CALLS. gmsh writes the mesh.
/// getdp writes a view with 1 tetrahedron and a complex vector, or, when `case.pro` holds FAIL,
/// fails as GetDP does.
const FAKE_GMSH: &str = "#!/bin/sh\necho gmsh \"$@\" >> CALLS\n: > case.msh\n";
const FAKE_GETDP: &str = r#"#!/bin/sh
echo getdp "$@" >> CALLS
if grep -q FAIL case.pro; then echo 'Error   : Unknown Resolution (Nope)'; exit 1; fi
printf 'View "j" {\nVS(0,0,0,1,0,0,0,1,0,0,0,1){1,0,0,1,0,0,1,0,0,1,0,4,0,0,8,0,0,8,0,0,8,0,0,8};\nTIME{0,1};\n};\n' > j.pos
"#;

/// A GetDP case with `text` in `case.pro` and a `case.geo`, and the fake programs.
fn getdp_lab(name: &str, text: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let case = dir.join("case");
    fs::create_dir_all(&case).unwrap();
    fs::write(case.join("case.pro"), text).unwrap();
    fs::write(case.join("case.geo"), "Point(1) = {0, 0, 0};\n").unwrap();
    let calls = p(&dir.join("calls")).to_owned();
    script(&dir.join("gmsh"), &FAKE_GMSH.replace("CALLS", &calls));
    script(&dir.join("getdp"), &FAKE_GETDP.replace("CALLS", &calls));
    (dir, case)
}

fn fh_run_getdp(dir: &std::path::Path, case: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(case)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_GMSH", dir.join("gmsh"))
        .env("FH_GETDP", dir.join("getdp"))
        .output()
        .unwrap()
}

/// `fh run CASE` meshes `case.geo` with gmsh, solves `case.pro` with getdp, and writes each step
/// of the views to a VTU file of the page: the real part, then the imaginary part.
#[test]
fn fh_run_solves_a_getdp_case_with_gmsh_then_getdp() {
    let (dir, case) = getdp_lab("run-getdp", "Resolution {}\n");
    let out = fh_run_getdp(&dir, &case);
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");
    assert!(
        shown.contains("  program gmsh\n  program getdp\n"),
        "{shown}"
    );
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    assert_eq!(
        calls,
        "gmsh case.geo -3 -o case.msh\ngetdp case.pro -msh case.msh -solve #1 -pos #1\n"
    );
    let run = dir.join("state/runs/run-000001");
    assert!(run.join("out/getdp/log.getdp").is_file(), "{shown}");
    let copy = run.join("in/case.pro");
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    assert!(
        !dir.join("state/previews").exists(),
        "a case has no preview"
    );
    let pvd = fs::read_to_string(run.join("out/results.pvd")).unwrap();
    assert!(pvd.contains("step-2.vtu"), "{pvd}");
    for (step, want) in [(1, [1.0, 0.0, 1.0]), (2, [0.0, 0.0, 8.0])] {
        let vtu = fs::read(run.join(format!("out/step-{step}.vtu"))).unwrap();
        let grid = fh_core::vtu::read_vtu(&vtu).unwrap();
        assert_eq!((grid.points.len(), grid.cell_types.clone()), (4, vec![10]));
        assert!(grid.point_data.is_empty());
        let j = grid.cell_field("j").unwrap();
        assert_eq!((j.components, j.as_f64()), (3, Some(&want[..])));
    }
    let _ = fs::remove_dir_all(&dir);
}

/// A case with no `case.geo` uses its own `case.msh`. A program that fails ends the run with
/// its error lines and the path of its log.
#[test]
fn a_failed_getdp_fails_the_run_with_its_error_lines() {
    let (dir, case) = getdp_lab("run-getdp-fail", "FAIL\n");
    fs::remove_file(case.join("case.geo")).unwrap();
    let out = fh_run_getdp(&dir, &case);
    let errors = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{errors}");
    assert!(
        errors.contains(
            "getdp failed (exit status: 1): Error   : Unknown Resolution (Nope). Correct"
        ),
        "{errors}"
    );
    assert!(errors.contains("out/getdp/log.getdp"), "{errors}");
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    assert_eq!(calls, "getdp case.pro -msh case.msh -solve #1 -pos #1\n");
    let _ = fs::remove_dir_all(&dir);
}

/// The half MBB beam of `nx` x `ny` square elements, as `validation/cases/topology-mbb-beam`
/// writes it, with no print request.
fn mbb_beam(nx: u32, ny: u32) -> String {
    let node = |i: u32, j: u32| i * (ny + 1) + j + 1;
    let mut text = String::from("*NODE\n");
    for i in 0..=nx {
        for j in 0..=ny {
            text += &format!("{}, {i}, {j}\n", node(i, j));
        }
    }
    text += "*ELEMENT, TYPE=CPE4, ELSET=BEAM\n";
    for i in 0..nx {
        for j in 0..ny {
            let corners = [
                node(i, j),
                node(i + 1, j),
                node(i + 1, j + 1),
                node(i, j + 1),
            ];
            text += &format!("{}, {:?}\n", i * ny + j + 1, corners).replace(['[', ']'], "");
        }
    }
    text += "*NSET, NSET=SYMMETRY\n";
    for j in 0..=ny {
        text += &format!("{},\n", node(0, j));
    }
    text + &format!(
        "*BOUNDARY\nSYMMETRY, 1, 1\n{}, 2, 2\n*STEP\n*STATIC\n*CLOAD\n{}, 2, -1.0\n*END STEP\n",
        node(nx, 0),
        node(0, ny)
    )
}

/// The real ccx solves each iteration of a small topology case. The loop stops when no density
/// changes by more than 0.01, the compliance falls, and the page gets the density of each element.
#[test]
fn fh_run_optimizes_a_topology_case_with_ccx_in_each_iteration() {
    let Some(solver) = ccx_solver() else { return };
    let dir = scratch("run-topology");
    let case = dir.join("case");
    fs::create_dir_all(&case).unwrap();
    fs::write(case.join("job.inp"), mbb_beam(12, 4)).unwrap();
    let settings = "volume = 0.5\nradius = 1.5\nyoung = 1.0\npoisson = 0.3\n";
    fs::write(case.join("topology.toml"), settings).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(&case)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_CCX", &solver)
        .output()
        .unwrap();
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");
    assert!(shown.contains("\n  iteration 1 compliance "), "{shown}");

    let run = dir.join("state/runs/run-000001");
    let work = run.join("out/topology");
    let paths = format!(
        "fh run: text results {0}/history.txt (each iteration), {0}/solve/ccx/job.dat (the last \
         solve)",
        work.display()
    );
    assert_eq!(shown.lines().last(), Some(paths.as_str()), "{shown}");
    assert!(run.join("in/topology.toml").is_file());
    assert!(dir.join("state/previews/1/input.inp").is_file());
    let history = fs::read_to_string(work.join("history.txt")).unwrap();
    let rows: Vec<Vec<f64>> = (history.lines().skip(1))
        .map(|l| l.split_whitespace().map(|w| w.parse().unwrap()).collect())
        .collect();
    assert!(history.starts_with("# iteration compliance volume change\n"));
    let (first, last) = (&rows[0], &rows[rows.len() - 1]);
    assert!(rows.len() > 5 && last[3] <= 0.01, "{history}");
    assert!(last[1] < 0.5 * first[1], "{history}");

    let grid = fh_core::vtu::read_vtu(&fs::read(run.join("out/step-1.vtu")).unwrap()).unwrap();
    let density = grid
        .cell_field("density")
        .unwrap()
        .as_f64()
        .unwrap()
        .to_vec();
    assert_eq!(density.len(), 48);
    // The page draws only 3D cells: the 2D elements go to the page as hexahedra (VTK type 12).
    assert_eq!(grid.cell_types, vec![12; 48]);
    assert!(
        density.iter().all(|x| (0.0..=1.0).contains(x)),
        "{density:?}"
    );
    let mean = density.iter().sum::<f64>() / 48.0;
    assert!((mean - last[2]).abs() < 1e-6, "{mean} {history}");
    assert!(grid.point_field("U").is_some());
    let _ = fs::remove_dir_all(&dir);
}

/// A fake Python with PyChrono. It writes its arguments to CALLS, and the view `U` of 1 bar that
/// turns about its first end. When `chrono.py` holds FAIL, it fails as a Python with no PyChrono.
const FAKE_PYTHON: &str = r#"#!/bin/sh
echo python "$@" >> CALLS
if grep -q FAIL chrono.py; then echo "ModuleNotFoundError: No module named 'pychrono'" >&2; exit 1; fi
printf 'View "U" {\nVL(0,0,0,1,0,0){0,0,0,0,0,0,0,0,0,-1,1,0};\nTIME{0,0.5};\n};\n' > motion.pos
"#;

fn fh_run_chrono(name: &str, text: &str) -> (PathBuf, std::process::Output) {
    let dir = scratch(name);
    let case = dir.join("case");
    fs::create_dir_all(&case).unwrap();
    fs::write(case.join("chrono.py"), text).unwrap();
    let calls = p(&dir.join("calls")).to_owned();
    script(&dir.join("python"), &FAKE_PYTHON.replace("CALLS", &calls));
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(&case)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_PYCHRONO", dir.join("python"))
        .output()
        .unwrap();
    (dir, out)
}

/// `fh run CASE` starts `chrono.py` with the Python of PyChrono, and writes each step of the
/// view `U` to a VTU file of the page as the displacement, in SI-m.
#[test]
fn fh_run_solves_a_chrono_case_with_python() {
    let (dir, out) = fh_run_chrono("run-chrono", "import pychrono\n");
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");
    assert!(shown.contains("  program python\n"), "{shown}");
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    assert_eq!(calls, "python chrono.py\n");
    let run = dir.join("state/runs/run-000001");
    assert!(run.join("out/chrono/log.python").is_file(), "{shown}");
    let copy = run.join("in/chrono.py");
    assert!(fs::metadata(&copy).unwrap().permissions().readonly());
    assert!(!dir.join("state/previews").exists());
    let pvd = fs::read_to_string(run.join("out/results.pvd")).unwrap();
    assert!(
        pvd.contains("units=\"SI-m\" kind=\"multibody\"") && pvd.contains("step-2.vtu"),
        "{pvd}"
    );
    let vtu = fs::read(run.join("out/step-2.vtu")).unwrap();
    let grid = fh_core::vtu::read_vtu(&vtu).unwrap();
    assert_eq!(grid.points, vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
    assert!(grid.cell_data.is_empty());
    let u = grid.point_field("U").unwrap();
    assert_eq!(u.as_f64(), Some(&[0.0, 0.0, 0.0, -1.0, 1.0, 0.0][..]));
    let _ = fs::remove_dir_all(&dir);
}

/// A Python with no PyChrono ends the run with the install step and the path of the log.
#[test]
fn a_python_with_no_pychrono_fails_the_run_with_the_install_step() {
    let (dir, out) = fh_run_chrono("run-chrono-fail", "FAIL\n");
    let errors = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{errors}");
    assert!(
        errors.contains("has no PyChrono. Install PyChrono (README.md) and set FH_PYCHRONO"),
        "{errors}"
    );
    assert!(errors.contains("out/chrono/log.python"), "{errors}");
    let _ = fs::remove_dir_all(&dir);
}

/// A fake `lmp`. It writes its arguments to CALLS and a dump of 2 particles 0.5 apart in 2
/// snapshots, or, when `case.lmp` holds FAIL, fails as LAMMPS does.
const FAKE_LMP: &str = r#"#!/bin/sh
echo lmp "$@" >> CALLS
if grep -q FAIL case.lmp; then
  printf 'ERROR: Unknown command: FAIL (../input.cpp:314)\nLast input line: FAIL\n'; exit 1
fi
for t in 0 0.5; do
  printf 'ITEM: TIME\n%s\nITEM: TIMESTEP\n0\nITEM: NUMBER OF ATOMS\n2\n' $t
  printf 'ITEM: BOX BOUNDS ff ff pp\n0 1\n0 1\n-0.1 0.1\nITEM: ATOMS id x y z speed\n'
  printf '1 %s 0 0 1\n2 %s 0.5 0 2\n' $t $t
done > particles.dump
"#;

/// A LAMMPS case with `text` in `case.lmp`, and the fake `lmp`.
fn lammps_lab(name: &str, text: &str) -> (PathBuf, PathBuf) {
    let dir = scratch(name);
    let case = dir.join("case");
    fs::create_dir_all(&case).unwrap();
    fs::write(case.join("case.lmp"), text).unwrap();
    let calls = p(&dir.join("calls")).to_owned();
    script(&dir.join("lmp"), &FAKE_LMP.replace("CALLS", &calls));
    (dir, case)
}

fn fh_run_lammps(dir: &std::path::Path, case: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(case)])
        .env("FH_STATE", dir.join("state"))
        .env("FH_LAMMPS", dir.join("lmp"))
        .output()
        .unwrap()
}

/// `fh run CASE` runs `case.lmp` with lmp and writes each snapshot of the dump to a VTU file of
/// the page: each particle is a cube at its position in the snapshot.
#[test]
fn fh_run_solves_a_lammps_case_and_shows_the_particles() {
    let (dir, case) = lammps_lab("run-lammps", "run 10\n");
    let out = fh_run_lammps(&dir, &case);
    let (shown, errors) = (stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{shown}{errors}");
    assert!(shown.contains("  program lmp\n"), "{shown}");
    let calls = fs::read_to_string(dir.join("calls")).unwrap();
    assert_eq!(calls, "lmp -in case.lmp -log none\n");
    let run = dir.join("state/runs/run-000001");
    assert!(run.join("out/lammps/log.lmp").is_file(), "{shown}");
    assert!(fs::metadata(run.join("in/case.lmp"))
        .unwrap()
        .permissions()
        .readonly());
    let pvd = fs::read_to_string(run.join("out/results.pvd")).unwrap();
    assert!(
        pvd.contains("timestep=\"0.5\" part=\"0\" file=\"step-2.vtu\""),
        "{pvd}"
    );
    let vtu = fs::read(run.join("out/step-2.vtu")).unwrap();
    let grid = fh_core::vtu::read_vtu(&vtu).unwrap();
    assert_eq!(grid.cell_types, vec![12, 12]);
    // The side of a cube is 0.5, the distance of the particles. Particle 2 is at (0.5, 0.5, 0).
    assert_eq!(grid.points[8], [0.25, 0.25, -0.25]);
    assert_eq!(grid.points[14], [0.75, 0.75, 0.25]);
    let speed = grid.cell_field("speed").unwrap();
    assert_eq!(speed.as_f64(), Some(&[1.0, 2.0][..]));
    assert!(grid.cell_field("id").is_none() && grid.point_data.is_empty());
    let _ = fs::remove_dir_all(&dir);
}

/// A LAMMPS error ends the run with the error line, the line of the script and the log path.
#[test]
fn a_failed_lammps_fails_the_run_with_its_error_lines() {
    let (dir, case) = lammps_lab("run-lammps-fail", "FAIL\n");
    let out = fh_run_lammps(&dir, &case);
    let errors = stderr(&out);
    assert_eq!(out.status.code(), Some(1), "{errors}");
    assert!(
        errors.contains(
            "lmp failed (exit status: 1): ERROR: Unknown command: FAIL (../input.cpp:314) | \
             Last input line: FAIL. Correct case.lmp"
        ),
        "{errors}"
    );
    assert!(errors.contains("out/lammps/log.lmp"), "{errors}");
    let _ = fs::remove_dir_all(&dir);
}
