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
