use std::process::{Command, Output};

const PLANNED: [&str; 6] = ["results", "plot", "serve", "init", "team", "skills"];

fn fh(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(args)
        .output()
        .expect("run fh")
}

#[test]
fn help_prints_usage_and_planned_subcommands_and_exits_0() {
    for flag in ["--help", "-h"] {
        let out = fh(&[flag]);
        assert_eq!(out.status.code(), Some(0), "exit code for {flag}");
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(stdout.starts_with("usage: fh"), "usage line for {flag}");
        for name in PLANNED {
            let line = format!("  {name:<10}not implemented");
            assert!(stdout.contains(&line), "missing `{line}` for {flag}");
        }
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

#[test]
fn planned_subcommand_is_reported_as_not_implemented_and_exits_2() {
    let out = fh(&["results"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("fh results: not implemented"));
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
    assert!(!out.contains("check     not implemented"));
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

#[test]
fn validate_reports_no_solver_for_every_case() {
    let out = fh(&["validate", "--root", &root()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = text(out.stdout);
    assert!(stdout.starts_with("no solver"), "{stdout}");
    for id in ["cantilever", "lame-cylinder", "patch-test"] {
        assert!(
            stdout.contains(&format!("{id}\t")),
            "{id} missing in {stdout}"
        );
    }
    assert!(
        stdout.contains("3 cases: 0 pass, 0 fail, 3 no solver"),
        "{stdout}"
    );
}

#[test]
fn validate_exits_1_when_the_case_folder_is_bad() {
    let dir = std::env::temp_dir().join(format!("fh-cli-val-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("validation/cases/x")).unwrap();
    std::fs::write(dir.join("validation/cases/x/case.kv"), "id = x\n").unwrap();
    let out = fh(&["validate", "--root", dir.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(out.stderr).contains("case.kv"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn validate_rejects_an_unknown_option_with_exit_2() {
    assert_eq!(fh(&["validate", "--bogus"]).status.code(), Some(2));
}
