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

#[test]
fn missing_gmsh_gives_a_clear_error() {
    let step = fixture("block.step");
    fails(&mesh(&[p(&step)], no_gmsh()), 1, "cannot start gmsh");
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
    let step = fixture("block.step");
    fails(&fh_raw(&[p(&step)], no_gmsh()), 1, "unit.missing");
}

#[test]
fn the_units_are_checked_against_the_step_file() {
    let dir = scratch("units");
    let step = dir.join("a.step");
    std::fs::write(&step, METRE_STEP).unwrap();
    says(
        &fh_raw(&[p(&step), "--units", "SI-mm"], no_gmsh()),
        "unit.step.mismatch",
    );
    says(
        &fh_raw(&[p(&step), "--units", "SI-m"], no_gmsh()),
        "cannot start gmsh",
    );
}

#[test]
fn bad_options_exit_2() {
    for args in [
        &[][..],
        &["a.step", "--order", "2"],
        &["a.step", "--bogus"],
        &["a.step", "--case", "x"],
        &["a.step", "--refine"],
        &["a.step", "--refine", "A"],
        &["a.step", "--refine", "A=0"],
        &["a.step", "--refine", "A=x"],
        &["a.step", "--refine", "A=1", "--refine", "a=2"],
        &["a.step", "--refine", "A,A=1"],
    ] {
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
        let step = fixture("block.step");
        let out = dir.join("a.inp");
        let mut args = vec![p(&step), "-o", p(&out)];
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
        fails(&out, 1, "mesh.jacobian.negative: 1 elements");
    }

    #[test]
    fn gmsh_receives_explicit_arguments() {
        // Record the arguments, then fail. The record shows the explicit flags.
        let dir = scratch("args");
        let gmsh = script(&dir, "echo \"$@\" > \"$(dirname \"$0\")/args.txt\"; exit 1");
        let step = fixture("block.step");
        let out = dir.join("a.inp");
        mesh(&[p(&step), "--size", "0.5", "-o", p(&out)], Some(&gmsh));
        let args = std::fs::read_to_string(dir.join("args.txt")).unwrap();
        for flag in [
            "-3",
            "-nopopup",
            "-format msh41",
            "-order 2",
            "-clmax 0.5",
            "-v 2",
        ] {
            assert!(args.contains(flag), "missing `{flag}` in `{args}`");
        }
    }
}

#[test]
fn real_gmsh_names_the_parts_edges_and_nodes_of_a_surface_step() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("shell-step");
    let out = dir.join("strip.inp");
    let step = fixture("strip-shell.step");
    let base = [
        p(&step),
        "--shell",
        "--size",
        "0.5",
        "-o",
        p(&out),
        "--face",
        "FIXED=x:0",
        "--nodes",
        "TIP=point:4,0.5,0",
    ];
    // Two thicknesses need two element sets: --volume names a surface in a box.
    let mut args = base.to_vec();
    args.extend([
        "--volume",
        "LEFT=box:0,0,0,2,1,0",
        "--volume",
        "RIGHT=box:2,0,0,4,1,0",
    ]);
    let run = mesh(&args, Some(&gmsh));
    assert_eq!(run.status.code(), Some(0), "stderr: {}", stderr(&run));
    let inp = std::fs::read_to_string(&out).unwrap();
    for needle in [
        "TYPE=S6",
        "*ELSET, ELSET=LEFT",
        "*ELSET, ELSET=RIGHT",
        "*NSET, NSET=FIXED",
        "*NSET, NSET=TIP",
        "*SURFACE, TYPE=ELEMENT, NAME=FIXED",
    ] {
        assert!(inp.contains(needle), "{needle}\n{inp}");
    }
    // The edge of a shell is face S3 to S5. The fixed edge is the left edge x=0.
    let faces: Vec<&str> = inp
        .lines()
        .skip_while(|l| !l.starts_with("*SURFACE, TYPE=ELEMENT, NAME=FIXED"))
        .skip(1)
        .take_while(|l| !l.starts_with('*'))
        .collect();
    assert!(faces.len() >= 2, "{faces:?}");
    assert!(faces
        .iter()
        .all(|l| l.ends_with("S3") || l.ends_with("S4") || l.ends_with("S5")));
    // No --volume: the default group SHELL holds every surface, and it is not written.
    let one = mesh(&base, Some(&gmsh));
    assert_eq!(one.status.code(), Some(0), "stderr: {}", stderr(&one));
    let inp = std::fs::read_to_string(&out).unwrap();
    assert!(inp.contains("TYPE=S6") && !inp.contains("SHELL"), "{inp}");
    // A part with no box is lost, so the run is refused.
    let mut lost = base.to_vec();
    lost.extend(["--volume", "LEFT=box:0,0,0,2,1,0"]);
    fails(&mesh(&lost, Some(&gmsh)), 1, "mesh.volume.unnamed");
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
        "mesh.shell.solid",
    );
    fails(
        &mesh(&[p(&strip), "-o", p(&out)], Some(&gmsh)),
        1,
        "use --shell",
    );
    assert!(!out.exists());
}

#[test]
fn a_refine_that_names_no_face_or_has_no_size_is_refused_before_gmsh_starts() {
    let step = fixture("block.step");
    let base = [p(&step), "--face", "END=x:-2"];
    let run = |extra: &[&str]| mesh(&[&base[..], extra].concat(), no_gmsh());
    fails(
        &run(&["--size", "1", "--refine", "FAR=0.1"]),
        1,
        "there is no --face FAR",
    );
    fails(&run(&["--refine", "END=0.1"]), 1, "needs --size");
    fails(
        &run(&["--size", "1", "--refine", "END=1"]),
        1,
        "must be below --size 1",
    );
    // A good refine reaches the gmsh start.
    says(
        &run(&["--size", "1", "--refine", "end=0.1"]),
        "cannot start gmsh",
    );
}

/// The x of each node in an input that `fh mesh` wrote.
fn node_xs(inp: &str) -> Vec<f64> {
    inp.lines()
        .skip_while(|l| !l.starts_with("*NODE"))
        .skip(1)
        .take_while(|l| !l.starts_with('*'))
        .filter_map(|l| l.split(',').nth(1)?.trim().parse().ok())
        .collect()
}

/// The size of the node set `name` in the report of `fh mesh`.
fn set_size(report: &str, name: &str) -> usize {
    let prefix = format!("nset\t{name}\t");
    let line = report.lines().find_map(|l| l.strip_prefix(&prefix));
    line.expect(report).parse().unwrap()
}

#[test]
fn real_gmsh_makes_the_mesh_finer_at_a_refined_set_for_a_solid_and_a_shell() {
    let Some(gmsh) = real_gmsh() else {
        eprintln!("skipped: gmsh is not installed");
        return;
    };
    let dir = scratch("refine");
    let out = dir.join("a.inp");
    // The part, whether it is a shell, and the x of the plane of the refined set.
    for (file, shell, at) in [("block.step", false, -2.0), ("strip-shell.step", true, 0.0)] {
        let step = fixture(file);
        let face = format!("END=x:{at}");
        let mut args = vec![p(&step), "--size", "1", "--face", &face, "-o", p(&out)];
        if shell {
            args.push("--shell");
        }
        let run = |extra: &[&str]| {
            let done = mesh(&[&args[..], extra].concat(), Some(&gmsh));
            assert_eq!(done.status.code(), Some(0), "{file}: {}", stderr(&done));
            (stdout(&done), std::fs::read_to_string(&out).unwrap())
        };
        let (plain_report, plain) = run(&[]);
        let (fine_report, fine) = run(&["--refine", "END=0.1"]);
        // Near the set the mesh has at least 3 times more nodes. Far from it, no more nodes.
        let near = |inp: &str| {
            node_xs(inp)
                .iter()
                .filter(|x| (*x - at).abs() < 0.5)
                .count()
        };
        let far = |inp: &str| {
            node_xs(inp)
                .iter()
                .filter(|x| (*x - at).abs() > 3.0)
                .count()
        };
        assert!(
            near(&fine) >= 3 * near(&plain),
            "{file}: {} {}",
            near(&fine),
            near(&plain)
        );
        assert!(
            far(&fine) <= far(&plain),
            "{file}: {} {}",
            far(&fine),
            far(&plain)
        );
        assert!(
            set_size(&fine_report, "END") >= 2 * set_size(&plain_report, "END"),
            "{file}"
        );
    }
}
