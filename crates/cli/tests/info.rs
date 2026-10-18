//! `fh info`: STEP unit refusals and mesh checks. The gmsh test skips when gmsh is missing.

mod common;
use common::*;
use std::path::Path;
use std::process::{Command, Output};

fn step_text(unit: &str) -> String {
    format!(
        "ISO-10303-21;\nDATA;\n#1=PRODUCT('p','p','',(#2));\n{unit}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

const MM: &str = "#9=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.) );";
const METRE: &str = "#9=( LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT($,.METRE.) );";
const NONE: &str = "#9=( NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.) );";

/// `fh info DIR/part.step --case DIR` for a STEP file with the unit record `unit`.
fn info_step(dir: &Path, unit: &str) -> Output {
    let file = dir.join("part.step");
    std::fs::write(&file, step_text(unit)).unwrap();
    fh(&["info", p(&file), "--case", p(dir)])
}

fn run_step(unit: &str, case_units: &str) -> Output {
    info_step(&patch_case(case_units), unit)
}

#[test]
fn matching_units_pass() {
    let out = run_step(MM, "SI-mm");
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = stdout(&out);
    assert!(
        stdout.contains("units_match: yes") && stdout.contains("gmsh_target_unit: MM"),
        "{stdout}"
    );
    assert_eq!(run_step(METRE, "SI-m").status.code(), Some(0));
}

#[test]
fn a_unit_mismatch_is_refused() {
    for (unit, case_units) in [(METRE, "SI-mm"), (MM, "SI-m")] {
        fails(&run_step(unit, case_units), 1, "unit.step.mismatch");
    }
}

#[test]
fn a_missing_unit_record_is_refused() {
    fails(&run_step(NONE, "SI-mm"), 1, "unit.step.missing");
}

#[test]
fn a_case_without_units_is_refused() {
    let dir = patch_case("SI-mm");
    let toml = std::fs::read_to_string(dir.join("case.toml")).unwrap();
    std::fs::write(
        dir.join("case.toml"),
        toml.replace("units = \"SI-mm\"\n", ""),
    )
    .unwrap();
    fails(&info_step(&dir, MM), 2, "missing key `units`");
}

#[test]
fn an_empty_mesh_is_refused() {
    let file = scratch("empty").join("empty.msh");
    std::fs::write(&file, "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n").unwrap();
    fails(&fh(&["info", p(&file)]), 1, "mesh.empty");
}

/// Run gmsh on a `.geo` text in `dir`.
fn run_gmsh(gmsh: &Path, dir: &Path, name: &str, geo: &str) {
    std::fs::write(dir.join(name), geo).unwrap();
    let out = Command::new(gmsh)
        .current_dir(dir)
        .args([name, "-0", "-nopopup"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stdout(&out));
}

#[test]
fn gmsh_recipe_gives_a_c3d10_bar_that_passes_the_gate() {
    let Some(gmsh) = real_gmsh() else {
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
    let case = patch_case("SI-mm");
    let out = fh(&["info", p(&dir.join("bar.step")), "--case", p(&case)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let out = fh(&["info", p(&dir.join("bar.msh")), "--case", p(&case)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let stdout = stdout(&out);
    assert!(stdout.contains("elements: C3D10="), "{stdout}");
    assert!(
        stdout.contains("bbox_max: [100.0, 20.0, 10.0] mm"),
        "{stdout}"
    );
    assert!(stdout.contains("volume: 2.000000e4"), "{stdout}");
}

/// A one-hex `.msh` file. `top` is the x coordinate of the top face corners 2 and 3.
fn hex_msh(top: f64) -> String {
    let xy = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut nodes = String::new();
    for (i, z) in [0.0, 1.0].into_iter().enumerate() {
        for (x, y) in xy {
            let x = if i == 1 && x == 1.0 { top } else { x };
            nodes.push_str(&format!("{x} {y} {z}\n"));
        }
    }
    format!(
        "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n1 8 1 8\n3 1 0 8\n1\n2\n3\n4\n5\n6\n7\n8\n{nodes}$EndNodes\n$Elements\n1 1 1 1\n3 1 5 1\n1 1 2 3 4 5 6 7 8\n$EndElements\n"
    )
}

#[test]
fn a_hex_mesh_is_accepted_only_when_it_is_an_extrusion() {
    let file = scratch("hex").join("hex.msh");
    std::fs::write(&file, hex_msh(1.0)).unwrap();
    let out = fh(&["info", p(&file)]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    std::fs::write(&file, hex_msh(1.5)).unwrap();
    fails(&fh(&["info", p(&file)]), 1, "mesh.hex.not_extruded");
}
