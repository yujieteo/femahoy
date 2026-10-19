//! Tests for the pass rule, the check file and the shipped cases.

use fh_validation::{check, load_all, Case, Field, FixtureError, Op};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases")
}

fn write_case(dir: &std::path::Path, body: &str) -> Result<Case, FixtureError> {
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
fn a_check_file_is_read() {
    let dir = tmp("ok");
    let text = body("").replace("rel_tol = 1\nabs_tol = 0", "rel_tol = 1e-6\nabs_tol = 1e-9");
    let c = write_case(&dir, &text).unwrap();
    assert_eq!((c.rel_tol, c.abs_tol), (1e-6, 1e-9));
    assert_eq!(c.input, dir.join("job.inp"));
    assert_eq!(c.id, dir.file_name().unwrap().to_string_lossy());
    let r = &c.references[0];
    assert_eq!((r.name.as_str(), r.value), ("q", 1.0));
    let x = &r.quantity;
    assert_eq!((x.field, x.component, x.set.as_str()), (Field::U, 1, "s"));
    assert_eq!(x.op, Op::Mean);
}

#[test]
fn every_field_component_and_op_is_accepted() {
    let d = tmp("fields");
    let fields = [
        ("U", Field::U, vec![1, 2, 3]),
        ("RF", Field::RF, vec![1, 2, 3]),
        ("S", Field::S, vec![11, 22, 33, 12, 13, 23]),
    ];
    let ops = [
        ("value", Op::Value),
        ("mean", Op::Mean),
        ("max", Op::Max),
        ("min", Op::Min),
        ("sum", Op::Sum),
    ];
    let mut k = 0;
    for (name, field, comps) in &fields {
        for comp in comps {
            for (word, op) in &ops {
                let text = body("")
                    .replace("field = \"U\"", &format!("field = \"{name}\""))
                    .replace("component = 1", &format!("component = {comp}"))
                    .replace("\"mean\"", &format!("\"{word}\""));
                k += 1;
                let c = write_case(&d.join(format!("f{k}")), &text).unwrap();
                let x = &c.references[0].quantity;
                assert_eq!((x.field, x.component, x.op), (*field, *comp, *op));
            }
        }
    }
}

#[test]
fn a_bad_check_file_is_refused_with_a_reason_and_a_line_when_known() {
    let d = tmp("bad");
    let cases = [
        (body("").replace("abs_tol = 0\n", ""), "abs_tol", None),
        (body("").replace("rel_tol = 1\n", ""), "rel_tol", None),
        ("rel_tol = 1\nabs_tol = 0\n".to_string(), "no check", None),
        (body("bogus = 1"), "`bogus` must be a table", Some(3)),
        (
            body("").replace("op = \"mean\"", "op = \"mean\"\nbogus = 1"),
            "unknown key `bogus` in `q`",
            Some(10),
        ),
        (
            body("").replace("rel_tol = 1", "rel_tol = \"x\""),
            "rel_tol",
            Some(1),
        ),
        (
            body("").replace("rel_tol = 1", "rel_tol = -1"),
            "rel_tol",
            Some(1),
        ),
        (
            format!("{}\nno equals sign", body("")),
            "check.toml:11",
            Some(11),
        ),
        (
            body("").replace("value = 1.0", "value = \"a\""),
            "`value` must be a number",
            Some(5),
        ),
        (
            body("").replace("value = 1.0\n", ""),
            "missing key `value`",
            Some(4),
        ),
        (
            body("").replace("field = \"U\"", "field = \"T\""),
            "unknown field `T`",
            Some(7),
        ),
        (
            body("").replace("component = 1", "component = 4"),
            "`component` must be one of [1, 2, 3]",
            Some(8),
        ),
        (
            body("")
                .replace("field = \"U\"", "field = \"S\"")
                .replace("component = 1", "component = 3"),
            "`component` must be one of [11, 22, 33, 12, 13, 23]",
            Some(8),
        ),
        (
            body("").replace("\"mean\"", "\"median\""),
            "unknown op `median`",
            Some(9),
        ),
        (
            body("").replace("set = \"s\"\n", ""),
            "missing key `set`",
            Some(4),
        ),
        (
            body("").replace("set = \"s\"", "set = \"\""),
            "`set` must be a non-empty string",
            Some(6),
        ),
    ];
    for (i, (text, needle, line)) in cases.iter().enumerate() {
        let err = write_case(&d.join(format!("c{i}")), text).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(needle), "case {i}: `{msg}` lacks `{needle}`");
        assert!(msg.contains("check.toml"), "case {i}: `{msg}`");
        assert_eq!(err.line, *line, "case {i}: `{msg}`");
    }
}

#[test]
fn a_case_without_an_input_is_refused() {
    let dir = tmp("no-input");
    write_case(&dir, &body("")).unwrap();
    std::fs::remove_file(dir.join("job.inp")).unwrap();
    let error = Case::load(&dir).unwrap_err();
    assert!(error.message.contains("job.inp not found"), "{error}");
}

#[test]
fn the_shipped_cases_load_and_have_their_inputs() {
    let cases = load_all(&root()).unwrap();
    let ids: Vec<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "bar-pressure",
            "bar-pressure-m",
            "bar-tension",
            "cantilever-c3d10",
            "kirsch-plate-c3d10",
            "lame-cylinder-c3d10",
            "plate-on-edge",
            "warpage-strip",
            "warpage-thick-copper"
        ]
    );
    for c in &cases {
        assert!(c.input.is_file(), "{}", c.input.display());
        assert!(!c.references.is_empty(), "{}", c.id);
    }
    let tip = &cases
        .iter()
        .find(|c| c.id == "cantilever-c3d10")
        .unwrap()
        .references[0];
    assert_eq!(tip.value, -0.01919619048);
    let q = &tip.quantity;
    assert_eq!((q.field, q.component, q.op), (Field::U, 3, Op::Mean));
    assert_eq!(q.set, "tip");
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
            assert!(error.message.contains(key), "{key}: {error}");
            assert_eq!(error.line, Some(line));
        }
    }
}
