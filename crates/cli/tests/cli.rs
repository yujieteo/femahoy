use std::process::{Command, Output};

const PLANNED: [&str; 8] = [
    "check", "validate", "results", "plot", "serve", "init", "team", "skills",
];

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
    let out = fh(&["check"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("fh check: not implemented"));
}
