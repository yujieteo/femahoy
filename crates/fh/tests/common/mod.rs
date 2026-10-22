//! Helpers that the `fh` test files share. Each file uses only some of them.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A new empty folder for one test.
pub fn scratch(name: &str) -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fh-test-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The path as an argument.
pub fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// Write an executable script at `path`.
pub fn script(path: &Path, text: &str) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_path_buf()
}

/// `ccx` (`FH_CCX`, else `ccx` on PATH) when it starts. Else `None` with a message, and the test
/// does not solve. With `FH_REQUIRE_CCX=1`, as in CI, a missing ccx fails the test.
pub fn ccx_solver() -> Option<PathBuf> {
    let ccx = PathBuf::from(std::env::var_os("FH_CCX").unwrap_or_else(|| "ccx".into()));
    let found = Command::new(&ccx).arg("-v").output().is_ok();
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_CCX").is_none_or(|v| v != "1"),
            "FH_REQUIRE_CCX=1, but ccx does not start"
        );
        eprintln!("skipped: ccx does not start. Set FH_CCX");
    }
    found.then_some(ccx)
}

/// `openfoam` (`FH_OPENFOAM`, else `openfoam` on PATH) when it starts. Else `None` with a
/// message, and the test does not solve. With `FH_REQUIRE_OPENFOAM=1`, as in CI, a missing
/// OpenFOAM fails the test.
pub fn openfoam_solver() -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os("FH_OPENFOAM").unwrap_or_else(|| "openfoam".into()));
    let found = Command::new(&exe).arg("-help").output().is_ok();
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_OPENFOAM").is_none_or(|v| v != "1"),
            "FH_REQUIRE_OPENFOAM=1, but openfoam does not start"
        );
        eprintln!("skipped: openfoam does not start. Set FH_OPENFOAM");
    }
    found.then_some(exe)
}

/// `getdp` (`FH_GETDP`, else `getdp` on PATH) when it starts. Else `None` with a message, and
/// the test does not solve. With `FH_REQUIRE_GETDP=1`, as in CI, a missing GetDP fails the test.
pub fn getdp_solver() -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os("FH_GETDP").unwrap_or_else(|| "getdp".into()));
    let found = Command::new(&exe).arg("-version").output().is_ok();
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_GETDP").is_none_or(|v| v != "1"),
            "FH_REQUIRE_GETDP=1, but getdp does not start"
        );
        eprintln!("skipped: getdp does not start. Set FH_GETDP");
    }
    found.then_some(exe)
}

/// The Python of PyChrono (`FH_PYCHRONO`, else `python3` on PATH) when it imports PyChrono. Else
/// `None` with a message, and the test does not solve. With `FH_REQUIRE_PYCHRONO=1`, as in CI, a
/// missing PyChrono fails the test.
pub fn pychrono_solver() -> Option<PathBuf> {
    python_with("FH_PYCHRONO", "pychrono", "PyChrono")
}

/// The Python of Meep (`FH_MEEP`, else `python3` on PATH) when it imports Meep, as for PyChrono.
/// `FH_REQUIRE_MEEP=1` makes a missing Meep fail the test.
pub fn meep_solver() -> Option<PathBuf> {
    python_with("FH_MEEP", "meep", "Meep")
}

/// The Python of `$<var>` (else `python3` on PATH) when it imports `module`, the module of the
/// solver `name`.
fn python_with(var: &str, module: &str, name: &str) -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os(var).unwrap_or_else(|| "python3".into()));
    let import = Command::new(&exe)
        .args(["-c", &format!("import {module}")])
        .output();
    let found = import.is_ok_and(|o| o.status.success());
    if !found {
        let require = format!("FH_REQUIRE_{}", name.to_uppercase());
        assert!(
            std::env::var_os(&require).is_none_or(|v| v != "1"),
            "{require}=1, but {} does not import {module}",
            exe.display()
        );
        eprintln!("skipped: no Python with {name}. Set {var}");
    }
    found.then_some(exe)
}

/// `ccx_preCICE` (`FH_CCX_PRECICE`, else `ccx_preCICE` on PATH) when it starts. Else `None` with
/// a message, and the test does not solve. With `FH_REQUIRE_PRECICE=1`, as in CI, a missing
/// `ccx_preCICE` fails the test.
pub fn precice_solver() -> Option<PathBuf> {
    let exe =
        PathBuf::from(std::env::var_os("FH_CCX_PRECICE").unwrap_or_else(|| "ccx_preCICE".into()));
    let found = Command::new(&exe).arg("-v").output().is_ok();
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_PRECICE").is_none_or(|v| v != "1"),
            "FH_REQUIRE_PRECICE=1, but ccx_preCICE does not start"
        );
        eprintln!("skipped: ccx_preCICE does not start. Set FH_CCX_PRECICE");
    }
    found.then_some(exe)
}

/// `lmp` (`FH_LAMMPS`, else `lmp` on PATH) when it starts. Else `None` with a message, and the
/// test does not solve. With `FH_REQUIRE_LAMMPS=1`, as in CI, a missing LAMMPS fails the test.
pub fn lammps_solver() -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os("FH_LAMMPS").unwrap_or_else(|| "lmp".into()));
    let found = Command::new(&exe).arg("-h").output().is_ok();
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_LAMMPS").is_none_or(|v| v != "1"),
            "FH_REQUIRE_LAMMPS=1, but lmp does not start"
        );
        eprintln!("skipped: lmp does not start. Set FH_LAMMPS");
    }
    found.then_some(exe)
}

/// The Python of openEMS (`FH_OPENEMS`, else `python3` on PATH) when it imports openEMS. Else
/// `None` with a message, and the test does not solve. With `FH_REQUIRE_OPENEMS=1`, as in CI, a
/// missing openEMS fails the test.
pub fn openems_solver() -> Option<PathBuf> {
    let exe = PathBuf::from(std::env::var_os("FH_OPENEMS").unwrap_or_else(|| "python3".into()));
    let import = Command::new(&exe)
        .args(["-c", "import CSXCAD, openEMS"])
        .output();
    let found = import.is_ok_and(|o| o.status.success());
    if !found {
        assert!(
            std::env::var_os("FH_REQUIRE_OPENEMS").is_none_or(|v| v != "1"),
            "FH_REQUIRE_OPENEMS=1, but {} does not import openEMS",
            exe.display()
        );
        eprintln!("skipped: no Python with openEMS. Set FH_OPENEMS");
    }
    found.then_some(exe)
}

pub fn cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fh"))
}

pub fn fh(args: &[&str]) -> Output {
    cmd().args(args).output().expect("run fh")
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Asserts that stderr holds `needle`.
#[track_caller]
pub fn says(out: &Output, needle: &str) {
    assert!(stderr(out).contains(needle), "{}", stderr(out));
}

/// Asserts the exit code `code` and that stderr holds `needle`.
#[track_caller]
pub fn fails(out: &Output, code: i32, needle: &str) {
    assert_eq!(out.status.code(), Some(code), "{}", stderr(out));
    says(out, needle);
}

/// The gmsh program that the real tests use, or `None` when it is not installed.
pub fn real_gmsh() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FH_GMSH").filter(|p| !p.is_empty()) {
        return Some(p.into());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("gmsh"))
        .find(|p| p.is_file())
}
