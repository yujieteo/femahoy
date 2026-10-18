mod common;
use common::{ccx_solver, p};
use std::process::{Command, Output};

/// A solver that does not exist. A test that does not solve uses it.
const NO_SOLVER: &str = "/nonexistent/solver";

fn fh(args: &[&str]) -> Output {
    fh_with_solver(args, NO_SOLVER)
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
fn help_lists_validate_and_not_check() {
    let out = text(fh(&["--help"]).stdout);
    assert!(!out.contains("  check     "));
    assert!(out.contains("  validate  "));
}

#[test]
fn validate_runs_every_case_through_ccx() {
    let Some(solver) = ccx_solver() else { return };
    let out = fh_with_solver(&["validate", "--root", &root()], p(&solver));
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(out.stderr));
    for id in [
        "bar-pressure",
        "bar-pressure-m",
        "bar-tension",
        "cantilever-c3d10",
        "kirsch-plate-c3d10",
        "lame-cylinder-c3d10",
        "plate-on-edge",
        "warpage-strip",
        "warpage-thick-copper",
    ] {
        let row = format!("{id}\tpass\n");
        assert!(stdout.contains(&row), "{id} missing in {stdout}");
    }
    assert!(stdout.contains("9 cases: 9 pass, 0 fail"), "{stdout}");
    // One row per quantity: computed, reference, tolerance, margin.
    assert!(stdout.contains("tip_deflection_z"), "{stdout}");
    assert!(stdout.contains("margin"), "{stdout}");
}

#[test]
fn without_ccx_validate_stops_and_tells_how_to_install_it() {
    // FH_SOLVE is not set, so the solver is fh-solve-ccx. A path to it needs ccx also.
    for solver in [None, Some("/somewhere/fh-solve-ccx")] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fh"));
        cmd.args(["validate", "--root", &root()])
            .env("FH_CCX", "/nonexistent/ccx");
        match solver {
            Some(path) => cmd.env("FH_SOLVE", path),
            None => cmd.env_remove("FH_SOLVE"),
        };
        let out = cmd.output().unwrap();
        let (stdout, stderr) = (text(out.stdout), text(out.stderr));
        assert_eq!(out.status.code(), Some(1), "{stdout}{stderr}");
        assert!(stderr.contains("cannot start ccx"), "{stderr}");
        assert!(stderr.contains("docs/ccx.md"), "{stderr}");
        assert!(!stdout.contains("pass"), "{stdout}");
    }
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
    let dir = copy_case("badref", "bar-tension");
    let path = dir.join("validation/cases/bar-tension/case.toml");
    let text_in = std::fs::read_to_string(&path).unwrap();
    let broken = text_in.replacen("s_xx = 210.0", "s_xx = 300.0", 1);
    assert_ne!(broken, text_in);
    std::fs::write(&path, broken).unwrap();
    let out = fh_with_solver(&["validate", "--root", p(&dir)], p(&solver));
    let stdout = text(out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("bar-tension\tfail"), "{stdout}");
    assert!(stdout.contains("1 cases: 0 pass, 1 fail"), "{stdout}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The runner stops a case at its `time_limit_s` and at its `memory_limit_mib`.
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
            if path.is_dir() && !path.ends_with("bar-tension") {
                std::fs::remove_dir_all(path).unwrap();
            }
        }
        let path = cases.join("bar-tension/case.toml");
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
            stdout.contains("bar-tension\tcannot run") && stdout.contains(needle),
            "{stdout}"
        );
        assert!(clock.elapsed() < std::time::Duration::from_secs(20));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn validate_exits_1_when_the_solver_is_missing() {
    let out = fh(&["validate", "--root", &root()]);
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

#[test]
fn validate_checks_each_quantity_against_the_meshes() {
    // The shipped cases pass these checks: validate_runs_every_case_through_ccx.
    // Each edit of the cantilever case: the text, its replacement and the expected error.
    let cases: [(&str, &str, &str); 2] = [
        ("set = \"tip\"", "set = \"nope\"", "no node set `nope`"),
        ("step = \"tip-load\"", "step = \"other\"", "no step `other`"),
    ];
    for (i, (from, to, needle)) in cases.iter().enumerate() {
        let dir = copy_root(&format!("spec{i}"));
        let path = dir.join("validation/cases/cantilever-c3d10/case.toml");
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
