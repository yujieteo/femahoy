mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::Output;

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/step").join(name)
}

fn mesh(args: &[&str], gmsh: Option<&Path>) -> Output {
    let mut cmd = cmd();
    cmd.arg("mesh").args(args);
    match gmsh {
        Some(p) => cmd.env("FH_GMSH", p),
        None => cmd.env_remove("FH_GMSH"),
    };
    cmd.output().expect("run fh")
}

/// A gmsh program that does not exist.
fn no_gmsh() -> Option<&'static Path> {
    Some(Path::new("/nonexistent/gmsh"))
}

#[test]
fn real_gmsh_makes_a_tet10_mesh_that_the_reader_accepts() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("real");
    let out = dir.join("block.inp");
    let step = fixture("block.step");
    let run = mesh(
        &[
            p(&step),
            "--size",
            "0.5",
            "-o",
            p(&out),
            "--face",
            "FIXED=x:-2",
        ],
        Some(&gmsh),
    );
    let stdout = stdout(&run);
    assert_eq!(run.status.code(), Some(0), "stderr: {}", stderr(&run));
    let inp = std::fs::read_to_string(&out).unwrap();
    assert!(inp.contains("*ELEMENT, TYPE=C3D10, ELSET=ALL"), "{inp}");
    assert!(inp.contains("*NSET, NSET=FIXED"), "no FIXED set");
    assert!(inp.contains("** units: SI-mm"), "{inp}");
    assert!(stdout.contains("ok\t"), "{stdout}");
    assert!(stdout.contains("nset\tFIXED\t"), "{stdout}");
    // The input holds the mesh and nothing else. The chat adds the model.
    assert!(
        !inp.contains("*STEP") && !inp.contains("*MATERIAL"),
        "{inp}"
    );
}

/// The upper layer of `layers.step` is 0.07 thick at z = 1.08, and 1.08 + 0.07 is 1.1500000000000001.
/// OpenCASCADE also makes each bounding box 1e-7 larger. The box of the layer must still select it.
#[test]
fn a_volume_box_selects_a_body_with_a_rounded_coordinate() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("rounded-layer");
    let out = dir.join("layers.inp");
    let step = fixture("layers.step");
    let run = mesh(
        &[
            p(&step),
            "--size",
            "1",
            "-o",
            p(&out),
            "--volume",
            "LOW=box:0,0,0,2,2,1.08",
            "--volume",
            "UP=box:0,0,1.08,2,2,1.15",
        ],
        Some(&gmsh),
    );
    let stdout = stdout(&run);
    assert_eq!(run.status.code(), Some(0), "stderr: {}", stderr(&run));
    assert!(stdout.contains("elset\tLOW\t"), "{stdout}");
    assert!(stdout.contains("elset\tUP\t"), "{stdout}");
}

#[test]
fn step_file_without_a_unit_record_is_refused() {
    let dir = scratch("nounit");
    let step = dir.join("a.step");
    std::fs::write(&step, "DATA;\nENDSEC;\n").unwrap();
    fails(&mesh(&[p(&step)], no_gmsh()), 1, "no length-unit record");
}

#[test]
fn a_step_file_in_metres_passes_the_unit_check() {
    let dir = scratch("metre");
    let step = dir.join("a.step");
    let text = "DATA;\n#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\nENDSEC;\n";
    std::fs::write(&step, text).unwrap();
    says(&mesh(&[p(&step)], no_gmsh()), "cannot start gmsh");
}

#[test]
fn a_shell_run_refuses_a_solid_and_a_solid_run_refuses_a_surface() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("shell-refusals");
    let (block, strip) = (fixture("block.step"), fixture("strip-shell.step"));
    let out = dir.join("a.inp");
    fails(
        &mesh(&[p(&block), "--shell", "-o", p(&out)], Some(&gmsh)),
        1,
        "needs surfaces",
    );
    fails(
        &mesh(&[p(&strip), "-o", p(&out)], Some(&gmsh)),
        1,
        "use --shell",
    );
    assert!(!out.exists());
}
