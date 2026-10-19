mod common;
use common::{ccx_solver, p};
use std::process::{Command, Output};

/// A ccx that does not exist. A test that does not solve uses it.
const NO_SOLVER: &str = "/nonexistent/ccx";

fn fh(args: &[&str]) -> Output {
    fh_with_solver(args, NO_SOLVER)
}

fn fh_with_solver(args: &[&str], solver: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(args)
        .env("FH_CCX", solver)
        .output()
        .expect("run fh")
}

fn root() -> String {
    format!("{}/../..", env!("CARGO_MANIFEST_DIR"))
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

#[test]
fn validate_runs_every_case_through_ccx() {
    let Some(solver) = ccx_solver() else { return };
    let out = fh_with_solver(&["validate", "--root", &root()], p(&solver));
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(out.stderr));
    for id in [
        "bar-pressure",
        "buckling-column",
        "contact-hertz",
        "dynamics-bar-step",
        "frequency-cantilever",
        "heat-fin",
        "plasticity-sphere",
        "scordelis-lo-roof",
        "warpage-2p5d-frame",
        "warpage-thick-copper",
    ] {
        let row = format!("{id}\tpass\n");
        assert!(stdout.contains(&row), "{id} missing in {stdout}");
    }
    assert!(stdout.contains("10 cases: 10 pass, 0 fail"), "{stdout}");
    // One row per quantity: computed, reference, tolerance, margin.
    assert!(stdout.contains("u_x_tip"), "{stdout}");
    assert!(stdout.contains("margin"), "{stdout}");
    warpage_matches_the_theory(&stdout);
}

/// The value `key=` in the table row of quantity `name` of case `id`.
fn row_value(table: &str, id: &str, name: &str, key: &str) -> f64 {
    let row = format!("  {id}\t{name}\t");
    let line = table.lines().find(|l| l.starts_with(&row)).unwrap();
    let field = line.split('\t').find_map(|f| f.strip_prefix(key)).unwrap();
    field.parse().unwrap()
}

/// The warpage reference is the measurement (see the derivation), and the copper slice agrees
/// within 1% with the plate theory for the same inputs.
fn warpage_matches_the_theory(table: &str) {
    // warpage-thick-copper: 0.700 um/K measured over 40 K (Calabretta et al. 2021, Table 2).
    let copper = row_value(table, "warpage-thick-copper", "warpage", "reference=");
    assert!((copper + 0.700e-3 * 40.0).abs() < 1e-12, "{copper}");
    // Biaxial bilayer curvature, Timoshenko form with plate moduli E / (1 - nu).
    let (m1, m2) = (120000.0 / (1.0 - 0.28), 77500.0 / (1.0 - 0.364));
    let (h1, h2): (f64, f64) = (0.73, 0.02);
    let strain = (1.64e-5 - 2.5e-6) * 40.0;
    let curvature = 6.0 * m1 * m2 * h1 * h2 * (h1 + h2) * strain
        / (m1 * m1 * h1.powi(4)
            + 4.0 * m1 * m2 * h1.powi(3) * h2
            + 6.0 * m1 * m2 * h1 * h1 * h2 * h2
            + 4.0 * m1 * m2 * h1 * h2.powi(3)
            + m2 * m2 * h2.powi(4));
    let theory = -curvature * 50.0 * 50.0 / 8.0;
    let computed = row_value(table, "warpage-thick-copper", "warpage", "computed=");
    assert!(
        (computed - theory).abs() <= 0.01 * theory.abs(),
        "{computed} {theory}"
    );
}

#[test]
fn without_ccx_validate_stops_and_tells_how_to_install_it() {
    let out = fh(&["validate", "--root", &root()]);
    let (stdout, stderr) = (text(out.stdout), text(out.stderr));
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    assert!(stderr.contains("cannot start ccx"), "{stderr}");
    assert!(stderr.contains("README.md"), "{stderr}");
    assert!(!stdout.contains("pass"), "{stdout}");
}

/// Like [`copy_root`], with only the case `id`, so that a solve stays short.
fn copy_case(tag: &str, id: &str) -> std::path::PathBuf {
    let dir = copy_root(tag);
    let cases = dir.join("validation/cases");
    for entry in std::fs::read_dir(&cases).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() && !path.ends_with(id) {
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    dir
}

#[test]
fn validate_exits_1_when_a_reference_is_wrong() {
    let Some(solver) = ccx_solver() else { return };
    let dir = copy_case("badref", "bar-pressure");
    let path = dir.join("validation/cases/bar-pressure/check.toml");
    let text_in = std::fs::read_to_string(&path).unwrap();
    let broken = text_in.replacen("value = 100.0", "value = 300.0", 1);
    assert_ne!(broken, text_in);
    std::fs::write(&path, broken).unwrap();
    let out = fh_with_solver(&["validate", "--root", p(&dir)], p(&solver));
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("bar-pressure\tfail"), "{stdout}");
    assert!(stdout.contains("1 cases: 0 pass, 1 fail"), "{stdout}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_exits_1_when_the_case_folder_is_bad() {
    let dir = std::env::temp_dir().join(format!("fh-cli-val-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("validation/cases/x")).unwrap();
    std::fs::write(dir.join("validation/cases/x/check.toml"), "id = \"x\"\n").unwrap();
    let out = fh(&["validate", "--root", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("check.toml"));
    let _ = std::fs::remove_dir_all(dir);
}

/// Copy the shipped case folders to a scratch root, so a test can break one of them.
fn copy_root(tag: &str) -> std::path::PathBuf {
    let dst = std::env::temp_dir().join(format!("fh-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    let from = std::path::Path::new(&root()).join("validation/cases");
    for entry in walk(&from) {
        let to = dst
            .join("validation/cases")
            .join(entry.strip_prefix(&from).unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &to).unwrap();
    }
    dst
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
