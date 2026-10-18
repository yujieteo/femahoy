mod common;
use common::{p, scratch};
use std::process::{Command, Output};

fn solver_path() -> std::path::PathBuf {
    let fh = std::path::Path::new(env!("CARGO_BIN_EXE_fh"));
    fh.with_file_name(format!("fh-solve{}", std::env::consts::EXE_SUFFIX))
}

fn fh(args: &[&str]) -> Output {
    fh_with_solver(args, solver_path().to_str().unwrap())
}

fn fh_with_solver(args: &[&str], solver: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(args)
        .env("FH_SOLVE", solver)
        .output()
        .expect("run fh")
}

#[test]
fn help_prints_usage_and_exits_0() {
    for flag in ["--help", "-h"] {
        let out = fh(&[flag]);
        assert_eq!(out.status.code(), Some(0), "exit code for {flag}");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.starts_with("usage: fh"), "usage line for {flag}");
    }
}

#[test]
fn unknown_argument_prints_usage_to_stderr_and_exits_2() {
    let out = fh(&["--bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "nothing on stdout");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown argument: --bogus"));
    assert!(stderr.contains("usage: fh"));
}

fn root() -> String {
    format!("{}/../..", env!("CARGO_MANIFEST_DIR"))
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

#[test]
fn help_lists_check_and_validate_as_commands() {
    let out = text(fh(&["--help"]).stdout);
    assert!(out.contains("  check     "));
    assert!(out.contains("  validate  "));
}

#[test]
fn check_parses_a_given_file() {
    let file = format!("{}/fixtures/inp/patch-test.inp", root());
    let out = fh(&["check", &file]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(out.stdout);
    assert!(stdout.contains("ok"), "{stdout}");
    assert!(stdout.contains("16 nodes, 7 elements"), "{stdout}");
}

#[test]
fn check_reports_a_parse_error_and_exits_1() {
    let bad = std::env::temp_dir().join(format!("fh-cli-bad-{}.inp", std::process::id()));
    std::fs::write(&bad, "*node\nabc, 0, 0, 0\n").unwrap();
    let out = fh(&["check", bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("fail"), "error line on stderr");
    let _ = std::fs::remove_file(bad);
}

#[test]
fn check_without_files_checks_every_fixture_under_root() {
    let out = fh(&["check", "--root", &root()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(out.stdout);
    for name in [
        "patch-test.inp",
        "cantilever.inp",
        "mesh-16.inp",
        "mesh-16x8.inp",
    ] {
        assert!(stdout.contains(name), "{name} missing in {stdout}");
    }
    assert!(stdout.contains("0 failed"), "{stdout}");
}

#[test]
fn check_with_a_missing_file_exits_1() {
    assert_eq!(fh(&["check", "/nonexistent/x.inp"]).status.code(), Some(1));
}

/// True when `ccx` (`FH_CCX`, else `ccx` on PATH) starts. The `bimetal-strip` case needs it.
fn ccx_found() -> bool {
    let exe = std::env::var_os("FH_CCX").unwrap_or_else(|| "ccx".into());
    let found = Command::new(exe).arg("-v").output().is_ok();
    if !found {
        eprintln!("ccx not found: bimetal-strip must be skipped. Set FH_CCX to run it");
    }
    found
}

#[test]
fn validate_runs_every_case_through_the_solver() {
    let out = fh(&["validate", "--root", &root()]);
    let stdout = text(out.stdout);
    // FH_SOLVE names fh-solve. The case bimetal-strip uses fh-solve-ccx all the same.
    // Without ccx, the case is skipped and the other cases decide the exit code.
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(out.stderr));
    if ccx_found() {
        assert!(stdout.contains("bimetal-strip\tN\tpass"), "{stdout}");
        assert!(stdout.contains("warpage-strip\tN\tpass"), "{stdout}");
        assert!(stdout.contains("warpage-thick-copper\tN\tpass"), "{stdout}");
        assert!(stdout.contains("plate-on-edge\tN\tpass"), "{stdout}");
        assert!(stdout.contains("20 cases: 20 pass, 0 fail"), "{stdout}");
    } else {
        assert!(
            stdout.contains("bimetal-strip\tN\tskipped, no ccx"),
            "{stdout}"
        );
        assert!(
            stdout.contains("plate-on-edge\tN\tskipped, no ccx"),
            "{stdout}"
        );
        assert!(stdout.contains("20 cases: 16 pass, 0 fail"), "{stdout}");
    }
    for id in [
        "bar-pressure",
        "bar-pressure-m",
        "bar-tension",
        "cantilever",
        "cantilever-c3d10",
        "cantilever-c3d8",
        "kirsch-plate-c3d10",
        "lame-cylinder",
        "lame-cylinder-c3d10",
        "lame-cylinder-c3d8i",
        "mms-c3d10",
        "mms-c3d8",
        "mms-c3d8i",
        "patch-test",
        "patch-test-c3d10",
        "patch-test-c3d8i",
    ] {
        let row = format!("{id}\t");
        assert!(stdout.contains(&row), "{id} missing in {stdout}");
    }
    assert!(stdout.contains("mms-c3d10\trate\t"), "{stdout}");
    assert!(stdout.contains("errors="), "{stdout}");
    assert!(!stdout.contains("no solver"), "{stdout}");
    // One row per quantity: computed, reference, tolerance, margin.
    assert!(stdout.contains("tip_deflection_z"), "{stdout}");
    assert!(stdout.contains("margin"), "{stdout}");
}

#[test]
fn without_ccx_a_ccx_case_is_skipped_and_the_other_cases_still_fail() {
    let dir = copy_root("noccx");
    let path = dir.join("validation/cases/patch-test/case.toml");
    let toml = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, toml.replacen("u9_x = 0.000325", "u9_x = 0.5", 1)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["validate", "--root", dir.to_str().unwrap()])
        .env("FH_SOLVE", solver_path())
        .env("FH_CCX", "/nonexistent/ccx")
        .output()
        .unwrap();
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("bimetal-strip\tN\tskipped, no ccx"),
        "{stdout}"
    );
    assert!(stdout.contains("patch-test\tN\tfail"), "{stdout}");
    assert!(stdout.contains("20 cases: 15 pass, 1 fail"), "{stdout}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_exits_1_when_a_reference_is_wrong() {
    let dir = copy_root("badref");
    let path = dir.join("validation/cases/patch-test/case.toml");
    let text_in = std::fs::read_to_string(&path).unwrap();
    let broken = text_in.replacen("u9_x = 0.000325", "u9_x = 0.5", 1);
    assert_ne!(broken, text_in);
    std::fs::write(&path, broken).unwrap();
    let out = fh(&["validate", "--root", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1), "{}", text(out.stdout));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_exits_1_when_a_class_r_rate_is_outside_the_band() {
    let dir = copy_root("badrate");
    let path = dir.join("validation/cases/mms-c3d8/case.toml");
    let text_in = std::fs::read_to_string(&path).unwrap();
    let broken = text_in.replacen("rate = 1.7", "rate = 4", 1);
    assert_ne!(broken, text_in);
    std::fs::write(&path, broken).unwrap();
    let case = dir.join("validation/cases/mms-c3d8");
    let out = fh(&["validate", "--convergence", case.to_str().unwrap()]);
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("mms-c3d8\tR\t3 meshes\tfail"), "{stdout}");
    assert!(text(out.stderr).contains("observed rate"), "stderr");
    let _ = std::fs::remove_dir_all(dir);
}

/// The runner stops a case at its `time_limit_s` and at its `memory_limit_mib`.
#[cfg(unix)]
#[test]
fn validate_stops_a_case_at_its_time_and_memory_limits() {
    use std::os::unix::fs::PermissionsExt;
    for (i, (limit, needle)) in [
        ("time_limit_s = 0.2", "time limit of 0.2 s"),
        ("memory_limit_mib = 0.001", "memory limit of 0.001 MiB"),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = copy_root(&format!("limit{i}"));
        let cases = dir.join("validation/cases");
        for entry in std::fs::read_dir(&cases).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && !path.ends_with("patch-test") {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
        let path = cases.join("patch-test/case.toml");
        let toml = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            toml.replacen("rel_tol", &format!("{limit}\nrel_tol"), 1),
        )
        .unwrap();
        // A solver that runs for 30 s.
        let slow = dir.join("slow-solver");
        std::fs::write(&slow, "#!/bin/sh\necho 'fh-solver-protocol 2'\nsleep 30\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let clock = std::time::Instant::now();
        let out = fh_with_solver(&["validate", "--root", p(&dir)], p(&slow));
        let stdout = text(out.stdout);
        assert_eq!(out.status.code(), Some(1), "{stdout}");
        assert!(
            stdout.contains("patch-test\tN\tcannot run") && stdout.contains(needle),
            "{stdout}"
        );
        assert!(clock.elapsed() < std::time::Duration::from_secs(20));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn validate_exits_1_when_the_solver_is_missing() {
    let out = fh_with_solver(&["validate", "--root", &root()], "/nonexistent/fh-solve");
    assert_eq!(out.status.code(), Some(1));
    let stdout = text(out.stdout);
    assert!(stdout.contains("cannot run"), "{stdout}");
    assert!(!stdout.contains("0 fail, 0 cannot"), "{stdout}");
}

#[test]
fn validate_exits_1_when_the_case_folder_is_bad() {
    let dir = std::env::temp_dir().join(format!("fh-cli-val-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("validation/cases/x")).unwrap();
    std::fs::write(dir.join("validation/cases/x/case.toml"), "id = \"x\"\n").unwrap();
    let out = fh(&["validate", "--root", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("case.toml"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_rejects_an_unknown_option_with_exit_2() {
    assert_eq!(fh(&["validate", "--bogus"]).status.code(), Some(2));
}

#[test]
fn convergence_on_a_class_l_case_runs_the_solver() {
    let case = format!("{}/validation/cases/cantilever", root());
    let out = fh(&["validate", "--convergence", &case]);
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(out.stderr));
    assert!(stdout.contains("cantilever\tL\t4 meshes\tpass"), "{stdout}");
    assert!(stdout.contains("order="), "{stdout}");
    assert!(stdout.contains("1 cases: 1 pass, 0 fail"), "{stdout}");
}

#[test]
fn convergence_on_the_case_folder_skips_class_n() {
    let dir = format!("{}/validation/cases", root());
    let out = fh(&["validate", "--convergence", &dir]);
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    for row in [
        "lame-cylinder\tL\t4 meshes\tpass",
        "patch-test\tN\tskipped, one mesh",
        "bar-tension\tN\tskipped, one mesh",
        "mms-c3d10\tR\t3 meshes\tpass",
        "bimetal-strip\tN\tskipped, one mesh",
        "20 cases: 10 pass, 0 fail",
    ] {
        assert!(stdout.contains(row), "{stdout}");
    }
}

#[test]
fn convergence_exits_1_when_the_solver_is_missing() {
    let case = format!("{}/validation/cases/cantilever", root());
    let out = fh_with_solver(
        &["validate", "--convergence", &case],
        "/nonexistent/fh-solve",
    );
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn convergence_on_a_class_n_case_exits_1() {
    let case = format!("{}/validation/cases/patch-test", root());
    let out = fh(&["validate", "--convergence", &case]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("class N"));
}

#[test]
fn convergence_needs_a_path_that_holds_cases() {
    assert_eq!(fh(&["validate", "--convergence"]).status.code(), Some(2));
    let out = fh(&["validate", "--convergence", "/nonexistent"]);
    assert_eq!(out.status.code(), Some(1));
}

/// Copy the shipped case folders to a scratch root, so a test can break one of them.
fn copy_root(tag: &str) -> std::path::PathBuf {
    let dst = std::env::temp_dir().join(format!("fh-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    for sub in ["validation/cases", "fixtures/inp"] {
        let from = std::path::Path::new(&root()).join(sub);
        for entry in walk(&from) {
            let to = dst.join(sub).join(entry.strip_prefix(&from).unwrap());
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(&entry, &to).unwrap();
        }
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

#[test]
fn validate_checks_each_quantity_against_the_meshes() {
    // The shipped cases pass (sets, steps and element types exist in every mesh).
    assert_eq!(fh(&["validate", "--root", &root()]).status.code(), Some(0));
    // Each edit of the cantilever case: the text, its replacement and the expected error.
    let cases: [(&str, &str, &str); 4] = [
        ("set = \"tip\"", "set = \"nope\"", "no node set `nope`"),
        ("step = \"tip-load\"", "step = \"other\"", "no step `other`"),
        ("[\"C3D8I\"]", "[\"C3D8\"]", "has element type C3D8I"),
        (
            "[\"C3D8I\"]",
            "[\"C3D8I\", \"C3D10\"]",
            "lists C3D10, but no mesh has that element type",
        ),
    ];
    for (i, (from, to, needle)) in cases.iter().enumerate() {
        let dir = copy_root(&format!("spec{i}"));
        let path = dir.join("validation/cases/cantilever/case.toml");
        let toml = std::fs::read_to_string(&path).unwrap();
        assert!(toml.contains(from), "{from}");
        std::fs::write(&path, toml.replace(from, to)).unwrap();
        let out = fh(&["validate", "--root", dir.to_str().unwrap()]);
        assert_eq!(out.status.code(), Some(1), "{needle}");
        let err = text(out.stderr);
        assert!(err.contains(needle), "`{err}` lacks `{needle}`");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// One hex8 element with no section.
const NO_SECTION: &str = "*node\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n*element, type=C3D8\n1,1,2,3,4,5,6,7,8\n";

/// A scratch root and its case folder `validation/cases/x`, which holds `mesh.inp` with
/// [`NO_SECTION`].
fn no_section_case(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = scratch(tag);
    let case = dir.join("validation/cases/x");
    std::fs::create_dir_all(&case).unwrap();
    std::fs::write(case.join("mesh.inp"), NO_SECTION).unwrap();
    (dir, case)
}

#[test]
fn check_fails_when_a_case_mesh_has_no_section() {
    let (dir, _) = no_section_case("nosec");
    let out = fh(&["check", "--root", p(&dir)]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(out.stderr);
    assert!(err.contains("no *SOLID SECTION"), "{err}");
    // The same file outside a case folder is only parsed.
    let loose = dir.join("loose.inp");
    std::fs::write(&loose, NO_SECTION).unwrap();
    assert_eq!(fh(&["check", p(&loose)]).status.code(), Some(0));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn check_fails_when_a_fixture_directory_is_unreadable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("fh-cli-unreadable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let hidden = dir.join("fixtures/hidden");
    std::fs::create_dir_all(&hidden).unwrap();
    let mesh = "*node\n1,0,0,0\n";
    std::fs::write(dir.join("fixtures/good.inp"), mesh).unwrap();
    std::fs::write(hidden.join("bad.inp"), mesh).unwrap();
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&hidden).is_ok(); // true when the test runs as root
    let out = fh(&["check", "--root", dir.to_str().unwrap()]);
    std::fs::set_permissions(&hidden, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = text(out.stderr);
    let code = out.status.code();
    let _ = std::fs::remove_dir_all(&dir);
    if readable {
        return;
    }
    assert_eq!(code, Some(2), "{err}");
    assert!(err.contains("hidden"), "{err}");
}

#[test]
fn check_finds_a_case_mesh_from_a_relative_path() {
    let (dir, case) = no_section_case("relsec");
    let absolute = case.join("mesh.inp");
    let mut bad = Vec::new();
    for path in [p(&absolute), "mesh.inp", "./mesh.inp", "../x/mesh.inp"] {
        let out = Command::new(env!("CARGO_BIN_EXE_fh"))
            .args(["check", path])
            .current_dir(&case)
            .output()
            .expect("run fh");
        let err = text(out.stderr);
        if out.status.code() != Some(1) || !err.contains("no *SOLID SECTION") {
            bad.push(format!("{path}: {:?} {err}", out.status.code()));
        } else if !err.contains(&format!("fail\t{path}\t")) {
            bad.push(format!("{path}: the message changes the path: {err}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn convergence_checks_the_fixture_as_validate_does() {
    let dir = copy_root("conv-spec");
    let case = dir.join("validation/cases/cantilever");
    let path = case.join("case.toml");
    let toml = std::fs::read_to_string(&path).unwrap();
    assert!(toml.contains("[\"C3D8I\"]"));
    std::fs::write(&path, toml.replace("[\"C3D8I\"]", "[\"C3D8\"]")).unwrap();
    for target in [case.clone(), dir.join("validation/cases")] {
        let out = fh(&["validate", "--convergence", target.to_str().unwrap()]);
        let err = text(out.stderr);
        assert_eq!(out.status.code(), Some(1), "{}: {err}", target.display());
        assert!(err.contains("has element type C3D8I"), "{err}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
