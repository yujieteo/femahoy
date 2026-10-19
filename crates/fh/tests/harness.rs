//! Tests for the pass rule, the check file and the shipped cases.

use fh::validation::{check, Case};
use std::path::PathBuf;

fn write_case(dir: &std::path::Path, body: &str) -> Result<Case, String> {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("check.toml"), body).unwrap();
    std::fs::write(dir.join("job.inp"), "*NODE\n1, 0, 0, 0\n").unwrap();
    Case::load(dir)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-validation-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A check file: `extra` top-level keys after the tolerances, then the check `q`.
fn body(extra: &str) -> String {
    format!(
        "rel_tol = 1\nabs_tol = 0\n{extra}\n[q]\nvalue = 1.0\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nop = \"mean\"\n"
    )
}

#[test]
fn the_pass_rule_uses_relative_plus_absolute_tolerance() {
    assert!(check(1.0000005, 1.0, 1e-6, 0.0).is_ok());
    assert!(check(1.00001, 1.0, 1e-6, 0.0).is_err());
    assert!(check(1e-12, 0.0, 0.0, 1e-9).is_ok());
}

#[test]
fn the_pass_rule_rejects_infinite_tolerance() {
    assert!(check(999.0, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn the_pass_rule_rejects_infinite_result() {
    assert!(check(f64::INFINITY, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn the_pass_rule_rejects_invalid_numbers_and_negative_tolerances() {
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..4 {
            let mut args = [1.0, 1.0, 0.0, 0.0];
            args[index] = bad;
            assert!(check(args[0], args[1], args[2], args[3]).is_err());
        }
    }
    assert!(check(1.0, 1.0, -1.0, 1.0).is_err());
    assert!(check(1.0, 1.0, 1.0, -1.0).is_err());
    assert!(check(1.0, 1.0, 0.0, 0.0).is_ok());
}

#[test]
fn the_pass_rule_rejects_non_finite_arithmetic() {
    assert!(check(1.0, f64::MAX, 2.0, 0.0).is_err());
    assert!(check(f64::MAX, -f64::MAX, 1.0, f64::MAX).is_err());
}

#[test]
fn a_number_that_is_not_finite_is_refused() {
    let dir = tmp("invalid-numbers");
    let mut index = 0;
    for (key, line) in [("rel_tol", 1), ("abs_tol", 2), ("value", 5)] {
        for bad in ["inf", "-inf", "nan"] {
            let text: Vec<String> = body("")
                .lines()
                .map(|l| {
                    if l.starts_with(&format!("{key} =")) {
                        format!("{key} = {bad}")
                    } else {
                        l.to_owned()
                    }
                })
                .collect();
            index += 1;
            let error = write_case(&dir.join(format!("bad{index}")), &text.join("\n")).unwrap_err();
            assert!(error.contains(key), "{key}: {error}");
            assert!(error.contains(&format!("check.toml:{line}: ")), "{error}");
        }
    }
}
