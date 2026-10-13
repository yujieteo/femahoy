use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BEAM: &str = "SetFactory(\"OpenCASCADE\");\n\
Box(1) = {0, 0, 0, 4, 1, 1};\n\
Physical Volume(\"BEAM\") = {1};\n\
Physical Surface(\"FIXED\") = {1};\n\
Physical Surface(\"LOAD\") = {2};\n\
Mesh.MeshSizeMax = 0.5;\n\
Mesh.MeshSizeMin = 0.5;\n";

/// A new empty folder for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fh-mesh-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fh(args: &[&str], gmsh: Option<&Path>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fh"));
    cmd.arg("mesh").args(args);
    match gmsh {
        Some(p) => cmd.env("FH_GMSH", p),
        None => cmd.env_remove("FH_GMSH"),
    };
    cmd.output().expect("run fh")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The gmsh program that the real test uses, or `None` when it is not installed.
fn real_gmsh() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FH_GMSH") {
        return Some(p.into());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("gmsh"))
        .find(|p| p.is_file())
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
    let out = fh(&[geo.to_str().unwrap()], Some(&gmsh));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(dir.join("beam.msh").is_file());
    assert!(stdout.contains("type\tC3D10\t"), "{stdout}");
    assert!(stdout.contains("elset\tBEAM\t"), "{stdout}");
    assert!(stdout.contains("nset\tFIXED\t"), "{stdout}");
    assert!(stdout.contains("order\t2"), "{stdout}");
}

#[test]
fn geo_file_with_system_call_is_refused_before_gmsh_starts() {
    let dir = scratch("syscall");
    for (i, text) in [
        "SystemCall \"echo x\";\n",
        "NonBlockingSystemCall \"echo x\";\n",
    ]
    .iter()
    .enumerate()
    {
        let geo = dir.join(format!("bad{i}.geo"));
        std::fs::write(&geo, text).unwrap();
        let out = fh(
            &[geo.to_str().unwrap()],
            Some(Path::new("/nonexistent/gmsh")),
        );
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("SystemCall"), "{}", stderr(&out));
        assert!(!dir.join(format!("bad{i}.msh")).exists());
    }
}

#[test]
fn missing_gmsh_gives_a_clear_error() {
    let dir = scratch("missing");
    let geo = dir.join("a.geo");
    std::fs::write(&geo, BEAM).unwrap();
    let out = fh(
        &[geo.to_str().unwrap()],
        Some(Path::new("/nonexistent/gmsh")),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("cannot start gmsh"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn step_file_without_a_unit_record_is_refused() {
    let dir = scratch("nounit");
    let step = dir.join("a.step");
    std::fs::write(&step, "DATA;\nENDSEC;\n").unwrap();
    let out = fh(
        &[step.to_str().unwrap()],
        Some(Path::new("/nonexistent/gmsh")),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unit.step.missing"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn step_unit_that_differs_from_the_case_units_is_refused() {
    let dir = scratch("mismatch");
    let step = dir.join("a.step");
    let text = "DATA;\n#1=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\nENDSEC;\n";
    std::fs::write(&step, text).unwrap();
    let gmsh = Some(Path::new("/nonexistent/gmsh"));
    let out = fh(&[step.to_str().unwrap()], gmsh);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("unit.step.mismatch"),
        "{}",
        stderr(&out)
    );
    // With the matching units the check passes and the run reaches the gmsh start.
    let out = fh(&[step.to_str().unwrap(), "--units", "SI-m"], gmsh);
    assert!(
        stderr(&out).contains("cannot start gmsh"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn bad_options_exit_2() {
    for args in [&[][..], &["a.geo", "--order", "3"], &["a.geo", "--bogus"]] {
        let out = fh(args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[cfg(unix)]
mod fake_gmsh {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A shell script that acts as gmsh. It answers `-version`, then runs `body`.
    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("gmsh");
        let text =
            format!("#!/bin/sh\nif [ \"$1\" = -version ]; then echo 4.15.2; exit 0; fi\n{body}\n");
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn run(name: &str, body: &str, extra: &[&str]) -> (Output, PathBuf) {
        let dir = scratch(name);
        let gmsh = script(&dir, body);
        let geo = dir.join("a.geo");
        std::fs::write(&geo, BEAM).unwrap();
        let mut args = vec![geo.to_str().unwrap()];
        args.extend(extra);
        (fh(&args, Some(&gmsh)), dir)
    }

    #[test]
    fn nonzero_exit_is_reported_with_the_error_lines() {
        let (out, _) = run("fails", "echo 'Error   : bad thing' >&2; exit 1", &[]);
        assert_eq!(out.status.code(), Some(1));
        let e = stderr(&out);
        assert!(
            e.contains("gmsh failed (exit code 1)") && e.contains("bad thing"),
            "{e}"
        );
    }

    #[test]
    fn error_line_with_exit_0_is_a_failure() {
        let (out, _) = run("pipwrapper", "echo 'Error   : bad thing' >&2; exit 0", &[]);
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("bad thing"), "{}", stderr(&out));
    }

    #[test]
    fn exit_0_without_a_mesh_file_is_a_failure() {
        let (out, _) = run("nomesh", "exit 0", &[]);
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("wrote no mesh"), "{}", stderr(&out));
    }

    #[test]
    fn mesh_file_that_the_reader_rejects_is_a_failure() {
        let body = "for a; do if [ \"$p\" = -o ]; then echo garbage > \"$a\"; fi; p=$a; done";
        let (out, _) = run("garbage", body, &[]);
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("not readable"), "{}", stderr(&out));
    }

    #[test]
    fn slow_gmsh_is_stopped_at_the_time_limit() {
        let start = std::time::Instant::now();
        let (out, _) = run("slow", "sleep 30", &["--timeout", "1"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr(&out).contains("timed out"), "{}", stderr(&out));
        assert!(start.elapsed().as_secs() < 20);
    }

    #[test]
    fn gmsh_receives_explicit_arguments() {
        // Record the arguments, then fail. The record shows the explicit flags.
        let dir = scratch("args");
        let gmsh = script(&dir, "echo \"$@\" > \"$(dirname \"$0\")/args.txt\"; exit 1");
        let geo = dir.join("a.geo");
        std::fs::write(&geo, BEAM).unwrap();
        fh(
            &[geo.to_str().unwrap(), "--size", "0.5", "--set", "h=2"],
            Some(&gmsh),
        );
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
