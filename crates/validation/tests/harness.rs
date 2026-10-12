//! Tests for the tolerance classes, the fixture schema and the shipped cases.

use fh_validation::{
    check_l, check_n, check_r, load_all, richardson, run_case, Case, Class, Outcome,
};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases")
}

fn write_case(dir: &std::path::Path, body: &str) -> Result<Case, fh_validation::FixtureError> {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("case.toml"), body).unwrap();
    std::fs::write(dir.join("m.inp"), "*node\n1, 0, 0, 0\n").unwrap();
    std::fs::write(dir.join("d.md"), "x\n").unwrap();
    Case::load(dir)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fh-validation-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A fixture body: the common keys, then `extra` top-level keys, then the `[ref]` table.
fn body(meshes: usize, extra: &str) -> String {
    let list = vec!["\"m.inp\""; meshes].join(", ");
    format!(
        "id = \"t\"\njustification = \"why\"\nderivation = \"d.md\"\nmesh = [{list}]\n{extra}\n[ref]\nq = 1.0\n"
    )
}

#[test]
fn richardson_recovers_second_order_limit() {
    // f(h) = 2 + 3 h^2 with h = 4, 2, 1 and ratio 2.
    let (limit, order, err) = richardson(&[50.0, 14.0, 5.0], 2.0).unwrap();
    assert!((limit - 2.0).abs() < 1e-12, "limit {limit}");
    assert!((order - 2.0).abs() < 1e-12, "order {order}");
    assert!((err - 3.0).abs() < 1e-12, "error estimate {err}");
}

#[test]
fn richardson_rejects_fewer_than_three_values_and_non_monotone_data() {
    assert!(richardson(&[1.0, 2.0], 2.0).is_err());
    assert!(richardson(&[1.0, 3.0, 2.0], 2.0).is_err());
}

#[test]
fn richardson_rejects_diverging_values() {
    // Differences grow towards the fine mesh: observed order is -1.
    let err = richardson(&[1.0, 1.1, 1.3], 2.0).unwrap_err();
    assert!(err.contains("order"), "{err}");
    assert!(check_l(&[1.0, 1.1, 1.3], 2.0, 1.3, 0.5).is_err());
}

#[test]
fn class_l_tolerance_is_discretisation_error_plus_reference_error() {
    // Fine value 5, estimated error 3, reference 7.5, stated reference error 0.5 (7.5 * 0.0667).
    let v = [50.0, 14.0, 5.0];
    assert!(check_l(&v, 2.0, 7.5, 0.5 / 7.5).is_ok()); // |5 - 7.5| = 2.5 <= 3 + 0.5
    assert!(check_l(&v, 2.0, 9.0, 0.0).is_err()); // 4 > 3
}

#[test]
fn class_n_uses_relative_plus_absolute_tolerance() {
    assert!(check_n(1.0000005, 1.0, 1e-6, 0.0).is_ok());
    assert!(check_n(1.00001, 1.0, 1e-6, 0.0).is_err());
    assert!(check_n(1e-12, 0.0, 0.0, 1e-9).is_ok());
}

#[test]
fn class_r_checks_observed_rate_inside_band() {
    // error = 0.5 h^2 on h = 0.4, 0.2, 0.1
    let h = [0.4, 0.2, 0.1];
    let e = [0.08, 0.02, 0.005];
    assert!(check_r(&h, &e, 2.0, 0.1).is_ok());
    assert!(check_r(&h, &e, 3.0, 0.1).is_err());
}

#[test]
fn schema_accepts_each_class() {
    let d = tmp("ok");
    let l = write_case(
        &d.join("l"),
        &body(3, "class = \"L\"\nratio = 2\nreference_error = 0.01"),
    )
    .unwrap();
    assert!(matches!(l.class, Class::L { .. }));
    let n = write_case(
        &d.join("n"),
        &body(1, "class = \"N\"\nrel_tol = 1e-6\nabs_tol = 1e-9"),
    )
    .unwrap();
    assert!(matches!(n.class, Class::N { .. }));
    let r = write_case(
        &d.join("r"),
        &body(2, "class = \"R\"\nrate = 2\nband = 0.2"),
    )
    .unwrap();
    assert!(matches!(r.class, Class::R { .. }));
    assert_eq!(r.references[0].name, "q");
    assert_eq!(r.references[0].value, 1.0);
}

#[test]
fn schema_rejects_bad_fixtures_with_a_reason_and_a_line_when_known() {
    let d = tmp("bad");
    let n = "class = \"N\"\nrel_tol = 1\nabs_tol = 0";
    let cases = [
        (
            body(1, "class = \"L\"\nratio = 2\nreference_error = 0"),
            "at least 3",
            Some(4),
        ),
        (body(2, n), "exactly 1", Some(4)),
        (body(1, "class = \"Z\""), "unknown class", Some(5)),
        (body(1, "class = \"N\""), "rel_tol", None),
        (
            "id = \"t\"\nclass = \"N\"\n[ref]\nq = 1\n".to_string(),
            "justification",
            None,
        ),
        (body(1, &format!("{n}\nbogus = 1")), "unknown key", Some(8)),
        (
            body(1, &format!("{n}\nratio = 2")),
            "`ratio` does not belong to class N",
            Some(8),
        ),
        (
            body(
                3,
                "class = \"L\"\nratio = 2\nreference_error = 0\nrel_tol = 1",
            ),
            "`rel_tol` does not belong to class L",
            Some(8),
        ),
        (body(1, n).replace("m.inp", "nope.inp"), "nope.inp", Some(4)),
        (
            body(1, "class = \"N\"\nrel_tol = \"x\"\nabs_tol = 0"),
            "rel_tol",
            Some(6),
        ),
        (
            format!("{}\nno equals sign", body(1, n)),
            "case.toml:11",
            Some(11),
        ),
        (
            body(1, n).replace("q = 1.0", "q = \"a\""),
            "reference `q`",
            Some(9),
        ),
        (body(1, n).replace("[ref]\nq = 1.0\n", ""), "[ref]", None),
    ];
    for (i, (text, needle, line)) in cases.iter().enumerate() {
        let err = write_case(&d.join(format!("c{i}")), text).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(needle), "case {i}: `{msg}` lacks `{needle}`");
        assert!(msg.contains("case.toml"), "case {i}: `{msg}`");
        assert_eq!(err.line, *line, "case {i}: `{msg}`");
    }
}

#[test]
fn shipped_cases_load_and_have_existing_inputs() {
    let cases = load_all(&root()).unwrap();
    let ids: Vec<_> = cases.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["cantilever", "lame-cylinder", "patch-test"]);
    for c in &cases {
        assert!(!c.references.is_empty() && !c.justification.is_empty());
        for m in &c.meshes {
            assert!(m.is_file(), "{}", m.display());
        }
    }
    let class = |id: &str| &cases.iter().find(|c| c.id == id).unwrap().class;
    assert!(matches!(class("cantilever"), Class::L { .. }));
    assert!(matches!(class("lame-cylinder"), Class::L { .. }));
    assert!(matches!(class("patch-test"), Class::N { .. }));
}

#[test]
fn run_case_reports_no_solver() {
    for c in load_all(&root()).unwrap() {
        assert_eq!(run_case(&c), Outcome::NoSolver, "{}", c.id);
    }
}
