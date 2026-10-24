mod common;
use common::{
    ccx_solver, getdp_solver, lammps_solver, meep_solver, openems_solver, openfoam_solver, p,
    precice_solver, pychrono_solver,
};
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
fn validate_runs_every_case() {
    let (
        Some(solver),
        Some(openfoam),
        Some(getdp),
        Some(precice),
        Some(pychrono),
        Some(lammps),
        Some(meep),
        Some(openems),
    ) = (
        ccx_solver(),
        openfoam_solver(),
        getdp_solver(),
        precice_solver(),
        pychrono_solver(),
        lammps_solver(),
        meep_solver(),
        openems_solver(),
    )
    else {
        return;
    };
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["validate", "--root", &root()])
        .env("FH_CCX", solver)
        .env("FH_OPENFOAM", openfoam)
        .env("FH_GETDP", getdp)
        .env("FH_PYCHRONO", pychrono)
        .env("FH_MEEP", meep)
        .env("FH_CCX_PRECICE", precice)
        .env("FH_LAMMPS", lammps)
        .env("FH_OPENEMS", openems)
        .output()
        .expect("run fh");
    let stdout = text(out.stdout);
    // Exit code 0: each case folder loaded and no case failed. The test has no count and no list
    // of the cases, because each new case would change them.
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(out.stderr));
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

/// Without ccx and OpenFOAM, each case runs and tells how to install its own solver.
#[test]
fn without_a_solver_each_case_tells_how_to_install_it() {
    let dir = copy_cases("nosolver", &["bar-pressure", "cavity-devahldavis"]);
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["validate", "--root", p(&dir)])
        .env("FH_CCX", NO_SOLVER)
        .env("FH_OPENFOAM", "/nonexistent/openfoam")
        .output()
        .expect("run fh");
    let (stdout, stderr) = (text(out.stdout), text(out.stderr));
    assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
    let tells = |id: &str, install: &str| {
        let row = format!("{id}\tcannot run");
        stdout
            .lines()
            .any(|l| l.starts_with(&row) && l.contains(install))
    };
    assert!(tells("bar-pressure", "Install CalculiX"), "{stdout}");
    assert!(tells("cavity-devahldavis", "Install OpenFOAM"), "{stdout}");
    assert!(stdout.contains("2 cases: 0 pass, 2 fail"), "{stdout}");
    let _ = std::fs::remove_dir_all(dir);
}

/// fh takes each solver from `solvers.env` beside its program, and a variable of the
/// environment has priority.
#[test]
fn fh_takes_each_solver_from_solvers_env_beside_it() {
    // A hard link, not a copy: a copy is open for write, and a test that forks in parallel
    // keeps it open, so its exec fails with "Text file busy". The target folder keeps the link
    // on the file system of the program.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fh-solvers-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fh = dir.join("fh");
    std::fs::hard_link(env!("CARGO_BIN_EXE_fh"), &fh).unwrap();
    std::fs::write(dir.join("solvers.env"), "FH_CCX=/nonexistent/file-ccx\n").unwrap();
    let model = format!("{}/validation/cases/bar-pressure/job.inp", root());
    let run = |ccx: Option<&str>| {
        let mut cmd = Command::new(&fh);
        cmd.args(["run", &model])
            .env("FH_STATE", dir.join("state"))
            .env_remove("FH_CCX");
        if let Some(ccx) = ccx {
            cmd.env("FH_CCX", ccx);
        }
        let out = cmd.output().expect("run fh");
        text(out.stdout) + &text(out.stderr)
    };
    let output = run(None);
    assert!(
        output.contains("cannot start /nonexistent/file-ccx"),
        "{output}"
    );
    let output = run(Some("/nonexistent/env-ccx"));
    assert!(
        output.contains("cannot start /nonexistent/env-ccx"),
        "{output}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Like [`copy_root`], with only the cases `ids`, so that a solve stays short.
fn copy_cases(tag: &str, ids: &[&str]) -> std::path::PathBuf {
    let dir = copy_root(tag);
    let cases = dir.join("validation/cases");
    for entry in std::fs::read_dir(&cases).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() && !ids.iter().any(|id| path.ends_with(id)) {
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    dir
}

#[test]
fn validate_exits_1_when_a_reference_is_wrong() {
    let Some(solver) = ccx_solver() else { return };
    let dir = copy_cases("badref", &["bar-pressure"]);
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
