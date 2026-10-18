mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::Output;

const BEAM: &str = "SetFactory(\"OpenCASCADE\");\n\
Box(1) = {0, 0, 0, 4, 1, 1};\n\
Physical Volume(\"BEAM\") = {1};\n\
Physical Surface(\"FIXED\") = {1};\n\
Physical Surface(\"LOAD\") = {2};\n\
Mesh.MeshSizeMax = 0.5;\n\
Mesh.MeshSizeMin = 0.5;\n";

/// Run `fh mesh`. The units are required (D-18), so this adds `--units SI-mm`
/// unless the arguments declare units.
fn mesh(args: &[&str], gmsh: Option<&Path>) -> Output {
    let declared = args.iter().any(|a| matches!(*a, "--units" | "--case"));
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
    let geo = dir.join("beam.geo");
    std::fs::write(&geo, BEAM).unwrap();
    let out = mesh(&[p(&geo)], Some(&gmsh));
    let stdout = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(dir.join("beam.msh").is_file());
    assert!(stdout.contains("type\tC3D10\t"), "{stdout}");
    assert!(stdout.contains("elset\tBEAM\t"), "{stdout}");
    assert!(stdout.contains("nset\tFIXED\t"), "{stdout}");
    assert!(stdout.contains("order\t2"), "{stdout}");
}

#[test]
fn geo_file_with_a_forbidden_command_is_refused_before_gmsh_starts() {
    let dir = scratch("syscall");
    for (i, text) in [
        "SystemCall \"echo x\";\n",
        "NonBlockingSystemCall \"echo x\";\n",
        "systemcall(\"echo x\");\n",
        "OnelabRun(\"x\");\n",
        "Include \"evil.geo\";\n",
        "Merge \"evil.geo\";\n",
        "Merge \"evil.pro\";\n",
        "Merge a;\n",
        "/* ok */ // ok\nSystemCall \"echo x\";\n",
    ]
    .iter()
    .enumerate()
    {
        let geo = dir.join(format!("bad{i}.geo"));
        std::fs::write(&geo, text).unwrap();
        let out = mesh(&[p(&geo)], no_gmsh());
        assert_eq!(out.status.code(), Some(1), "{text}: {}", stderr(&out));
        assert!(stderr(&out).contains("refuses"), "{text}: {}", stderr(&out));
        assert!(!dir.join(format!("bad{i}.msh")).exists());
    }
}

#[test]
fn geo_file_with_merge_msh_or_commented_keywords_reaches_gmsh() {
    let dir = scratch("allowed");
    for (i, text) in [
        "Merge \"part.MSH\";\n",
        "// SystemCall \"x\";\n/* Include \"a.geo\"; OnelabRun */\nPoint(1) = {0,0,0};\n",
        "Printf(\"// Include\");\n",
    ]
    .iter()
    .enumerate()
    {
        let geo = dir.join(format!("ok{i}.geo"));
        std::fs::write(&geo, text).unwrap();
        let out = mesh(&[p(&geo)], no_gmsh());
        assert!(
            !stderr(&out).contains("refuses"),
            "{text}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn missing_gmsh_gives_a_clear_error() {
    let dir = scratch("missing");
    let geo = dir.join("a.geo");
    std::fs::write(&geo, BEAM).unwrap();
    fails(&mesh(&[p(&geo)], no_gmsh()), 1, "cannot start gmsh");
}

#[test]
fn step_file_without_a_unit_record_is_refused() {
    let dir = scratch("nounit");
    let step = dir.join("a.step");
    std::fs::write(&step, "DATA;\nENDSEC;\n").unwrap();
    fails(&mesh(&[p(&step)], no_gmsh()), 1, "unit.step.missing");
}

#[test]
fn step_unit_that_differs_from_the_case_units_is_refused() {
    let dir = scratch("mismatch");
    let step = dir.join("a.step");
    std::fs::write(&step, METRE_STEP).unwrap();
    fails(&mesh(&[p(&step)], no_gmsh()), 1, "unit.step.mismatch");
    // With the matching units the check passes and the run reaches the gmsh start.
    says(
        &mesh(&[p(&step), "--units", "SI-m"], no_gmsh()),
        "cannot start gmsh",
    );
}

const METRE_STEP: &str = "DATA;\n#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\nENDSEC;\n";

#[test]
fn a_run_with_no_units_is_refused() {
    let dir = scratch("nounits");
    let geo = dir.join("a.geo");
    std::fs::write(&geo, BEAM).unwrap();
    fails(&fh_raw(&[p(&geo)], no_gmsh()), 1, "unit.case.missing");
}

#[test]
fn the_case_units_are_used_and_checked_against_the_step_file() {
    let dir = scratch("case");
    let step = dir.join("a.step");
    std::fs::write(&step, METRE_STEP).unwrap();
    let (mm, m) = (patch_case("SI-mm"), patch_case("SI-m"));
    says(
        &fh_raw(&[p(&step), "--case", p(&mm)], no_gmsh()),
        "unit.step.mismatch",
    );
    says(
        &fh_raw(&[p(&step), "--case", p(&m)], no_gmsh()),
        "cannot start gmsh",
    );
    // `--units` that differs from the case is a conflict, even for a .geo file.
    let args = [p(&step), "--case", p(&m), "--units", "SI-mm"];
    says(&fh_raw(&args, no_gmsh()), "unit.case.conflict");
}

/// A box that gmsh fills with hexes: a 2D grid, extruded along z, or a recombined 3D grid.
fn hex_geo(extruded: bool, taper: f64) -> String {
    let mut geo = String::from("SetFactory(\"OpenCASCADE\");\n");
    if extruded {
        geo.push_str(
            "Rectangle(1) = {0, 0, 0, 4, 1};\nTransfinite Curve {1, 3} = 5;\n\
Transfinite Curve {2, 4} = 2;\nTransfinite Surface {1};\nRecombine Surface {1};\n\
Extrude {0, 0, 1} { Surface{1}; Layers{2}; Recombine; }\n",
        );
    } else {
        // The top face is wider than the bottom face, so the lateral edges are not parallel.
        geo.push_str(&format!(
            "Point(1) = {{0,0,0}};\nPoint(2) = {{1,0,0}};\nPoint(3) = {{1,1,0}};\nPoint(4) = {{0,1,0}};\n\
Point(5) = {{0,0,1}};\nPoint(6) = {{{},0,1}};\nPoint(7) = {{{},1,1}};\nPoint(8) = {{0,1,1}};\n\
Line(1)={{1,2}};Line(2)={{2,3}};Line(3)={{3,4}};Line(4)={{4,1}};\n\
Line(5)={{5,6}};Line(6)={{6,7}};Line(7)={{7,8}};Line(8)={{8,5}};\n\
Line(9)={{1,5}};Line(10)={{2,6}};Line(11)={{3,7}};Line(12)={{4,8}};\n\
Curve Loop(1)={{1,2,3,4}};Curve Loop(2)={{5,6,7,8}};Curve Loop(3)={{1,10,-5,-9}};\n\
Curve Loop(4)={{2,11,-6,-10}};Curve Loop(5)={{3,12,-7,-11}};Curve Loop(6)={{4,9,-8,-12}};\n\
Plane Surface(1)={{1}};Plane Surface(2)={{2}};Plane Surface(3)={{3}};\n\
Plane Surface(4)={{4}};Plane Surface(5)={{5}};Plane Surface(6)={{6}};\n\
Surface Loop(1)={{1,2,3,4,5,6}};Volume(1)={{1}};\n\
Transfinite Curve {{1,2,3,4,5,6,7,8,9,10,11,12}} = 2;\n\
Transfinite Surface {{1,2,3,4,5,6}};Recombine Surface {{1,2,3,4,5,6}};\n\
Transfinite Volume {{1}};\n",
            1.0 + taper,
            1.0 + taper
        ));
    }
    geo.push_str("Physical Volume(\"BEAM\") = {1};\n");
    geo
}

fn hex_run(name: &str, geo: &str) -> Option<Output> {
    let gmsh = real_gmsh()?;
    let path = scratch(name).join("hex.geo");
    std::fs::write(&path, geo).unwrap();
    // Order 1 makes hex8. The default order 2 would make hex20, which the reader rejects.
    Some(mesh(&[p(&path), "--order", "1"], Some(&gmsh)))
}

#[test]
fn an_extruded_hex_mesh_is_accepted() {
    let Some(out) = hex_run("hex-ok", &hex_geo(true, 0.0)) else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("type\tC3D8I\t"), "{}", stdout(&out));
}

#[test]
fn a_hex_mesh_that_is_not_an_extrusion_is_refused() {
    let Some(out) = hex_run("hex-bad", &hex_geo(false, 0.4)) else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    assert_eq!(out.status.code(), Some(1), "stdout: {:?}", out.stdout);
    says(&out, "mesh.hex.not_extruded");
}

#[test]
fn bad_options_exit_2() {
    for args in [&[][..], &["a.geo", "--order", "3"], &["a.geo", "--bogus"]] {
        let out = mesh(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
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
        let geo = dir.join("a.geo");
        std::fs::write(&geo, BEAM).unwrap();
        let mut args = vec![p(&geo)];
        args.extend(extra);
        mesh(&args, Some(&gmsh))
    }

    #[test]
    fn nonzero_exit_is_reported_with_the_error_lines() {
        let out = run("fails", "echo 'Error   : bad thing' >&2; exit 1", &[]);
        assert_eq!(out.status.code(), Some(1));
        says(&out, "gmsh failed (exit code 1)");
        says(&out, "bad thing");
    }

    #[test]
    fn error_line_with_exit_0_is_a_failure() {
        let out = run("pipwrapper", "echo 'Error   : bad thing' >&2; exit 0", &[]);
        fails(&out, 1, "bad thing");
    }

    #[test]
    fn exit_0_without_a_mesh_file_is_a_failure() {
        fails(&run("nomesh", "exit 0", &[]), 1, "wrote no mesh");
    }

    #[test]
    fn mesh_file_that_the_reader_rejects_is_a_failure() {
        let body = "for a; do if [ \"$p\" = -o ]; then echo garbage > \"$a\"; fi; p=$a; done";
        fails(&run("garbage", body, &[]), 1, "not readable");
    }

    #[test]
    fn slow_gmsh_is_stopped_at_the_time_limit() {
        let start = std::time::Instant::now();
        fails(
            &run("slow", "sleep 30", &["--timeout", "1"]),
            1,
            "timed out",
        );
        assert!(start.elapsed().as_secs() < 20);
    }

    #[test]
    fn gmsh_receives_explicit_arguments() {
        // Record the arguments, then fail. The record shows the explicit flags.
        let dir = scratch("args");
        let gmsh = script(&dir, "echo \"$@\" > \"$(dirname \"$0\")/args.txt\"; exit 1");
        let geo = dir.join("a.geo");
        std::fs::write(&geo, BEAM).unwrap();
        mesh(&[p(&geo), "--size", "0.5", "--set", "h=2"], Some(&gmsh));
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        for flag in [
            "-3",
            "-nopopup",
            "-format msh41",
            "-order 2",
            "-clmax 0.5",
            "-setnumber h 2",
            "-v 2",
        ] {
            assert!(args.contains(flag), "missing `{flag}` in `{args}`");
        }
    }
}
