//! Helpers that the `fh` test files share. Each file uses only some of them.
#![allow(dead_code)]

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

/// A copy of the patch test case that declares `units`. Its `mesh` is an absolute path to the
/// shared fixture.
pub fn patch_case(units: &str) -> PathBuf {
    let dir = scratch(&format!("case-{units}"));
    let src = root().join("validation/cases/patch-test");
    for file in ["derivation.md", "case.toml"] {
        std::fs::copy(src.join(file), dir.join(file)).unwrap();
    }
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    let mesh = root().join("fixtures/inp/patch-test.inp");
    let toml = toml
        .replace("units = \"SI-mm\"", &format!("units = \"{units}\""))
        .replace("../../../fixtures/inp/patch-test.inp", p(&mesh));
    std::fs::write(dir.join("case.toml"), toml).unwrap();
    dir
}
