//! `fh info`: STEP unit refusals and mesh checks. The gmsh test skips when gmsh is missing.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

fn fh(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(args)
        .output()
        .expect("run fh")
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

fn case_dir(units: &str) -> PathBuf {
    let dir = scratch(&format!("case-{units}"));
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases/patch-test");
    for name in ["derivation.md", "case.toml"] {
        std::fs::copy(src.join(name), dir.join(name)).unwrap();
    }
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    let toml = toml.replace("units = \"SI-mm\"", &format!("units = \"{units}\""));
    // The patch test mesh is a shared fixture. Point `mesh` at it with an absolute path.
    let mesh = src.join("../../../fixtures/inp/patch-test.inp");
    let toml = toml.replace(
        "../../../fixtures/inp/patch-test.inp",
        mesh.to_str().unwrap(),
    );
    std::fs::write(dir.join("case.toml"), toml).unwrap();
    dir
}

fn scratch(name: &str) -> PathBuf {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let n = COUNT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fh-info-{}-{n}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn step_text(unit: &str) -> String {
    format!(
        "ISO-10303-21;\nDATA;\n#1=PRODUCT('p','p','',(#2));\n{unit}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

const MM: &str = "#9=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );";
const METRE: &str = "#9=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );";
const NONE: &str = "#9=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );";

fn run_step(unit: &str, case_units: &str) -> Output {
    let dir = case_dir(case_units);
    let file = dir.join("part.step");
    std::fs::write(&file, step_text(unit)).unwrap();
    fh(&[
        "info",
        file.to_str().unwrap(),
        "--case",
        dir.to_str().unwrap(),
    ])
}

#[test]
fn matching_units_pass() {
    let out = run_step(MM, "SI-mm");
    assert_eq!(out.status.code(), Some(0), "{}", text(out.stderr));
    let stdout = text(out.stdout);
    assert!(
        stdout.contains("units_match: yes") && stdout.contains("gmsh_target_unit: MM"),
        "{stdout}"
    );
    assert_eq!(run_step(METRE, "SI-m").status.code(), Some(0));
}

#[test]
fn a_unit_mismatch_is_refused() {
    for (unit, case_units) in [(METRE, "SI-mm"), (MM, "SI-m")] {
        let out = run_step(unit, case_units);
        assert_eq!(out.status.code(), Some(1));
        assert!(text(out.stderr).contains("unit.step.mismatch"));
    }
}

#[test]
fn a_missing_unit_record_is_refused() {
    let out = run_step(NONE, "SI-mm");
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("unit.step.missing"));
}

#[test]
fn a_case_without_units_is_refused() {
    let dir = case_dir("SI-mm");
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    std::fs::write(
        dir.join("case.toml"),
        toml.replace("units = \"SI-mm\"\n", ""),
    )
    .unwrap();
    let file = dir.join("part.step");
    std::fs::write(&file, step_text(MM)).unwrap();
    let out = fh(&[
        "info",
        file.to_str().unwrap(),
        "--case",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(out.stderr).contains("missing key `units`"));
}

#[test]
fn an_empty_mesh_is_refused() {
    let dir = scratch("empty");
    let file = dir.join("empty.msh");
    std::fs::write(&file, "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n").unwrap();
    let out = fh(&["info", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", text(out.stderr));
    assert!(text(out.stderr).contains("mesh.empty"));
}

fn gmsh() -> Option<PathBuf> {
    let name = std::env::var_os("FH_GMSH").unwrap_or_else(|| "gmsh".into());
    let ok = Command::new(&name)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then(|| PathBuf::from(name))
}

/// Run gmsh on a `.geo` text in `dir`.
fn run_gmsh(gmsh: &Path, dir: &Path, name: &str, geo: &str) {
    std::fs::write(dir.join(name), geo).unwrap();
    let out = Command::new(gmsh)
        .current_dir(dir)
        .args([name, "-0", "-nopopup"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(out.stdout));
}

#[test]
fn gmsh_recipe_gives_a_c3d10_bar_that_passes_the_gate() {
    let Some(gmsh) = gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("gmsh");
    let make = "SetFactory(\"OpenCASCADE\");\nBox(1) = {0, 0, 0, 100, 20, 10};\nCoherence;\nSave \"bar.step\";\n";
    run_gmsh(&gmsh, &dir, "make.geo", make);
    let template = include_str!("../../../scripts/cad/bar.geo.in");
    let geo = template
        .replace("@UNIT@", "MM")
        .replace("@STEP@", "bar.step")
        .replace("@SIZE@", "5")
        .replace("@OUT@", "bar.msh");
    run_gmsh(&gmsh, &dir, "bar.geo", &geo);
    let case = case_dir("SI-mm");
    let step = dir.join("bar.step");
    let out = fh(&[
        "info",
        step.to_str().unwrap(),
        "--case",
        case.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(out.stderr));
    let out = fh(&[
        "info",
        dir.join("bar.msh").to_str().unwrap(),
        "--case",
        case.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", text(out.stderr));
    let stdout = text(out.stdout);
    assert!(stdout.contains("elements: C3D10="), "{stdout}");
    assert!(
        stdout.contains("bbox_max: [100.0, 20.0, 10.0] mm"),
        "{stdout}"
    );
    assert!(stdout.contains("volume: 2.000000e4"), "{stdout}");
}
