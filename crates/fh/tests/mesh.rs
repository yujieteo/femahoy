mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::Output;

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/step").join(name)
}

/// Run `fh mesh`. The units are required, so this adds `--units SI-mm`
/// unless the arguments declare units.
fn mesh(args: &[&str], gmsh: Option<&Path>) -> Output {
    let declared = args.contains(&"--units");
    let mut all = args.to_vec();
    if !declared {
        all.extend(["--units", "SI-mm"]);
    }
    fh_raw(&all, gmsh)
}

fn fh_raw(args: &[&str], gmsh: Option<&Path>) -> Output {
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
    assert!(stdout.contains("min_scaled_jacobian\t0."), "{stdout}");
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
fn step_unit_that_differs_from_the_case_units_is_refused() {
    let dir = scratch("mismatch");
    let step = dir.join("a.step");
    std::fs::write(&step, METRE_STEP).unwrap();
    fails(&mesh(&[p(&step)], no_gmsh()), 1, "the STEP file uses");
    // With the matching units the check passes and the run reaches the gmsh start.
    says(
        &mesh(&[p(&step), "--units", "SI-m"], no_gmsh()),
        "cannot start gmsh",
    );
}

const METRE_STEP: &str = "DATA;\n#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\nENDSEC;\n";

#[test]
fn a_run_with_no_units_is_refused() {
    let step = fixture("block.step");
    fails(&fh_raw(&[p(&step)], no_gmsh()), 1, "the run has no units");
}

#[test]
fn the_units_are_checked_against_the_step_file() {
    let dir = scratch("units");
    let step = dir.join("a.step");
    std::fs::write(&step, METRE_STEP).unwrap();
    says(
        &fh_raw(&[p(&step), "--units", "SI-mm"], no_gmsh()),
        "the STEP file uses",
    );
    says(
        &fh_raw(&[p(&step), "--units", "SI-m"], no_gmsh()),
        "cannot start gmsh",
    );
}

mod fake_gmsh {
    use super::*;

    /// A shell script that acts as gmsh. It answers `-version`, then runs `body`.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let text =
            format!("#!/bin/sh\nif [ \"$1\" = -version ]; then echo 4.15.2; exit 0; fi\n{body}\n");
        super::common::script(&dir.join("gmsh"), &text)
    }

    fn run(name: &str, body: &str, extra: &[&str]) -> Output {
        let dir = scratch(name);
        let gmsh = script(&dir, body);
        let step = fixture("block.step");
        let out = dir.join("a.inp");
        let mut args = vec![p(&step), "-o", p(&out)];
        args.extend(extra);
        mesh(&args, Some(&gmsh))
    }

    #[test]
    fn mesh_with_an_inverted_element_is_refused() {
        // One C3D10 element with corners 2 and 3 swapped, so its Jacobian is negative.
        let msh = "$MeshFormat\n4.1 0 8\n$EndMeshFormat\n$Nodes\n1 10 1 10\n3 1 0 10\n\
1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n0 0 0\n1 0 0\n0 1 0\n0 0 1\n0.5 0 0\n\
0.5 0.5 0\n0 0.5 0\n0 0 0.5\n0 0.5 0.5\n0.5 0 0.5\n$EndNodes\n$Elements\n1 1 1 1\n\
3 1 11 1\n1 1 3 2 4 7 6 5 8 9 10\n$EndElements\n";
        let body = format!(
            "for a; do if [ \"$p\" = -o ]; then cat > \"$a\" <<'EOF'\n{msh}EOF\nfi; p=$a; done"
        );
        let out = run("inverted", &body, &[]);
        fails(
            &out,
            1,
            "1 elements have a corner with a Jacobian of 0 or less",
        );
    }
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
