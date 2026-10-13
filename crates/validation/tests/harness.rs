//! Tests for the tolerance classes, the fixture schema and the shipped cases.

use fh_validation::{
    check_l, check_n, check_r, load_all, richardson, run_case, run_convergence, Case, Class, Field,
    Op, Outcome,
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

/// The measured-quantity table of the reference `q`.
const QUANTITY: &str =
    "\n[quantity.q]\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nstep = \"st\"\nop = \"mean\"\n";

/// A fixture body: the schema version, the common keys, then `extra` top-level keys, then the
/// `[ref]` table and the `[quantity.q]` table.
fn body(meshes: usize, extra: &str) -> String {
    let list = vec!["\"m.inp\""; meshes].join(", ");
    format!(
        "schema = 2\nid = \"t\"\njustification = \"why\"\nderivation = \"d.md\"\nmesh = [{list}]\nelement_types = [\"C3D8\"]\n{extra}\n[ref]\nq = 1.0\n{QUANTITY}"
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
    assert!(check_l(&[1.0, 1.1, 1.3], 2.0, 1.3, 0.5, (1.0, 3.0)).is_err());
}

#[test]
fn class_l_compares_the_extrapolated_limit_with_the_reference() {
    // f(h) = 2 + 3 h^2 on h = 4, 2, 1: limit 2, order 2, estimated error 3.
    let v = [50.0, 14.0, 5.0];
    let band = (1.0, 3.0);
    // |limit - ref| <= estimate + reference_error * |ref|.
    assert!(check_l(&v, 2.0, 4.9, 0.0, band).is_ok()); // |2 - 4.9| = 2.9 <= 3
    assert!(check_l(&v, 2.0, 5.1, 0.0, band).is_err()); // 3.1 > 3
    assert!(check_l(&v, 2.0, 5.1, 0.02, band).is_ok()); // 3.1 <= 3 + 0.102
    assert!(check_l(&v, 2.0, -1.1, 0.0, band).is_err()); // 3.1 on the other side
}

#[test]
fn class_l_rejects_a_reference_on_the_far_side_of_the_limit() {
    // Review example 1: limit 1.05, estimate 0.05, fine value 1.0. The reference 0.951 is within
    // 0.05 of the fine value but 0.099 from the limit, so it must fail.
    let v = [0.6, 0.9, 1.0];
    let err = check_l(&v, 2.0, 0.951, 0.0, (1.0, 3.0)).unwrap_err();
    assert!(err.contains("limit"), "{err}");
}

#[test]
fn class_l_rejects_an_observed_order_outside_the_band() {
    // Review example 2: observed order 0.322 widens the estimate to 0.16, which would hide a
    // 5 percent gap. The order band rejects it.
    let v = [0.80, 0.85, 0.89];
    let err = check_l(&v, 2.0, 1.0, 0.0, (1.0, 3.0)).unwrap_err();
    assert!(err.contains("order"), "{err}");
    // Too high an order is also outside the band (data not yet in the asymptotic range).
    assert!(check_l(&[50.0, 14.0, 5.0], 2.0, 2.0, 0.0, (0.5, 1.5)).is_err());
    // Inside the band the same data passes.
    assert!(check_l(&[50.0, 14.0, 5.0], 2.0, 2.0, 0.0, (1.5, 2.5)).is_ok());
}

/// Tip deflection (cantilever) and radial displacements (Lame), finest three meshes of each case,
/// from the linear solver of PR 10 (head ba0937d) on these meshes with a `*SOLID SECTION` added.
const CANTILEVER: [f64; 4] = [
    -1.842951475459e-2,
    -1.885000696066e-2,
    -1.900460765644e-2,
    -1.905873665041e-2,
];
const LAME_INNER: [f64; 4] = [
    8.846324204265e-2,
    9.018210774186e-2,
    9.063857187314e-2,
    9.075473588634e-2,
];
const LAME_OUTER: [f64; 4] = [
    5.661257340233e-2,
    5.747200625187e-2,
    5.770023831757e-2,
    5.775832032411e-2,
];

#[test]
fn shipped_class_l_cases_pass_with_the_recorded_solver_values() {
    let cases = load_all(&root()).unwrap();
    let get = |id: &str, name: &str| {
        let c = cases.iter().find(|c| c.id == id).unwrap();
        let Class::L {
            ratio,
            reference_error,
            order_min,
            order_max,
        } = c.class
        else {
            panic!()
        };
        let r = c.references.iter().find(|r| r.name == name).unwrap().value;
        (ratio, r, reference_error, (order_min, order_max))
    };
    for (id, name, values) in [
        ("cantilever", "tip_deflection_z", CANTILEVER),
        ("lame-cylinder", "ur_inner", LAME_INNER),
        ("lame-cylinder", "ur_outer", LAME_OUTER),
    ] {
        let (ratio, r, e, band) = get(id, name);
        let res = check_l(&values, ratio, r, e, band);
        let (limit, order, est) = richardson(&values, ratio).unwrap();
        println!("{id} {name}: limit {limit:e} ref {r:e} order {order:.3} estimate {est:e}");
        assert!(res.is_ok(), "{id} {name}: {res:?}");
    }
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
        &body(
            3,
            "class = \"L\"\nratio = 2\nreference_error = 0.01\norder_min = 1\norder_max = 3",
        ),
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
    assert_eq!(r.element_types, ["C3D8"]);
    let x = &r.references[0].quantity;
    assert_eq!((x.field, x.component, x.set.as_str()), (Field::U, 1, "s"));
    assert_eq!((x.step.as_str(), x.op), ("st", Op::Mean));
}

#[test]
fn schema_accepts_every_field_component_and_op() {
    let d = tmp("fields");
    let fields = [
        ("U", Field::U, vec![1, 2, 3]),
        ("RF", Field::RF, vec![1, 2, 3]),
        ("S", Field::S, vec![11, 22, 33, 12, 23, 13]),
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
                let text = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0")
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
fn schema_rejects_bad_fixtures_with_a_reason_and_a_line_when_known() {
    let d = tmp("bad");
    let n = "class = \"N\"\nrel_tol = 1\nabs_tol = 0";
    let cases = [
        (
            body(1, "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 1\norder_max = 3"),
            "at least 3",
            Some(5),
        ),
        (body(2, n), "exactly 1", Some(5)),
        (body(1, "class = \"Z\""), "unknown class", Some(7)),
        (body(1, "class = \"N\""), "rel_tol", None),
        (
            "schema = 2\nid = \"t\"\nclass = \"N\"\n[ref]\nq = 1\n".to_string(),
            "justification",
            None,
        ),
        (body(1, &format!("{n}\nbogus = 1")), "unknown key", Some(10)),
        (
            body(1, &format!("{n}\nratio = 2")),
            "`ratio` does not belong to class N",
            Some(10),
        ),
        (
            body(
                3,
                "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 1\norder_max = 3\nrel_tol = 1",
            ),
            "`rel_tol` does not belong to class L",
            Some(12),
        ),
        (body(1, n).replace("m.inp", "nope.inp"), "nope.inp", Some(5)),
        (
            body(1, "class = \"N\"\nrel_tol = \"x\"\nabs_tol = 0"),
            "rel_tol",
            Some(8),
        ),
        (
            format!("{}\nno equals sign", body(1, n)),
            "case.toml:20",
            Some(20),
        ),
        (
            body(1, n).replace("q = 1.0", "q = \"a\""),
            "reference `q`",
            Some(11),
        ),
        (body(1, n).replace("[ref]\nq = 1.0\n", ""), "[ref]", None),
        (body(1, n).replace("schema = 2\n", ""), "schema", None),
        (
            body(1, n).replace("schema = 2", "schema = 1"),
            "schema version 1",
            Some(1),
        ),
        (
            body(1, n).replace(QUANTITY, ""),
            "no quantity for reference `q`",
            None,
        ),
        (
            format!(
                "{}\n[quantity.w]\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nstep = \"st\"\nop = \"max\"\n",
                body(1, n)
            ),
            "quantity `w` has no reference",
            Some(20),
        ),
        (
            body(1, n).replace("field = \"U\"", "field = \"T\""),
            "unknown field `T`",
            Some(15),
        ),
        (
            body(1, n).replace("component = 1", "component = 4"),
            "component 4 is not valid for field U",
            Some(16),
        ),
        (
            body(1, n)
                .replace("field = \"U\"", "field = \"S\"")
                .replace("component = 1", "component = 3"),
            "component 3 is not valid for field S",
            Some(16),
        ),
        (
            body(1, n).replace("\"mean\"", "\"median\""),
            "unknown op `median`",
            Some(18),
        ),
        (
            body(1, n).replace("set = \"s\"\n", ""),
            "missing key `set`",
            Some(13),
        ),
        (
            body(1, n).replace("set = \"s\"", "set = \"\""),
            "`set` must be a non-empty string",
            Some(14),
        ),
        (
            body(1, n).replace("step = \"st\"", "step = \"\""),
            "`step` must be a non-empty string",
            Some(17),
        ),
        (
            body(1, n).replace("op = \"mean\"", "op = \"mean\"\nbogus = 1"),
            "unknown key `bogus` in `quantity.q`",
            Some(19),
        ),
        (
            body(1, n).replace("element_types = [\"C3D8\"]\n", ""),
            "element_types",
            None,
        ),
        (
            body(1, n).replace("[\"C3D8\"]", "[]"),
            "`element_types` must be a non-empty array",
            Some(6),
        ),
        (
            body(1, n).replace("\"C3D8\"", "\"S4R\""),
            "unknown element type `S4R`",
            Some(6),
        ),
        (
            body(3, "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 3\norder_max = 1"),
            "order_min must be below order_max",
            Some(10),
        ),
        (
            body(3, "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 0\norder_max = 1"),
            "order_min must be above 0",
            Some(10),
        ),
        (
            body(3, "class = \"L\"\nratio = 2\nreference_error = 0"),
            "order_min",
            None,
        ),
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
fn schema_preserves_string_errors_and_class_error_order() {
    let dir = tmp("reader-errors");
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let cases = [
        (
            "set = \"s\"",
            "set = 1",
            "`set` must be a non-empty string",
            Some(14),
        ),
        (
            "field = \"U\"",
            "field = \"\"",
            "`field` must be a non-empty string",
            Some(15),
        ),
        (
            "op = \"mean\"",
            "op = false",
            "`op` must be a non-empty string",
            Some(18),
        ),
        (
            "class = \"N\"",
            "class = \"\"",
            "`class` must be a non-empty string",
            Some(7),
        ),
        (
            "class = \"N\"",
            "class = \"Z\"",
            "unknown class `Z`, expected L, N or R",
            Some(7),
        ),
        (
            "class = \"N\"",
            "class = \"L\"",
            "`rel_tol` does not belong to class L",
            Some(8),
        ),
    ];
    for (index, (old, new, message, line)) in cases.iter().enumerate() {
        let error =
            write_case(&dir.join(format!("c{index}")), &valid.replace(old, new)).unwrap_err();
        assert_eq!(error.message, *message);
        assert_eq!(error.line, *line);
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
    // Every reference of every shipped case says what to measure.
    for c in &cases {
        assert!(!c.element_types.is_empty(), "{}", c.id);
        for r in &c.references {
            let q = &r.quantity;
            assert!(
                !q.set.is_empty() && !q.step.is_empty(),
                "{} {}",
                c.id,
                r.name
            );
        }
    }
    let types = |id: &str| {
        cases
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .element_types
            .clone()
    };
    assert_eq!(types("cantilever"), ["C3D8I"]);
    assert_eq!(types("lame-cylinder"), ["C3D8"]);
    assert_eq!(types("patch-test"), ["C3D8"]);
    // Class L cases have 4 meshes. Their references and ratio stay as they were.
    for id in ["cantilever", "lame-cylinder"] {
        let c = cases.iter().find(|c| c.id == id).unwrap();
        assert_eq!(c.meshes.len(), 4, "{id}");
        assert!(
            matches!(c.class, Class::L { ratio, .. } if ratio == 2.0),
            "{id}"
        );
    }
    let tip = &cases
        .iter()
        .find(|c| c.id == "cantilever")
        .unwrap()
        .references[0];
    assert_eq!(tip.value, -0.01919619048);
    let q = &tip.quantity;
    assert_eq!((q.field, q.component, q.op), (Field::U, 3, Op::Mean));
    assert_eq!((q.set.as_str(), q.step.as_str()), ("tip", "tip-load"));
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

#[test]
fn convergence_needs_a_mesh_sequence_and_reports_no_solver() {
    for c in load_all(&root()).unwrap() {
        let out = run_convergence(&c);
        match c.class {
            Class::N { .. } => assert!(
                matches!(out, Outcome::Fail(ref m) if m.contains("class N")),
                "{}",
                c.id
            ),
            _ => assert_eq!(out, Outcome::NoSolver, "{}", c.id),
        }
    }
}

#[test]
fn class_n_rejects_infinite_tolerance() {
    assert!(check_n(999.0, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn class_n_rejects_infinite_result() {
    assert!(check_n(f64::INFINITY, 1.0, f64::INFINITY, 0.0).is_err());
}

#[test]
fn class_l_rejects_infinite_reference_error() {
    assert!(check_l(&[50.0, 14.0, 5.0], 2.0, 999.0, f64::INFINITY, (1.0, 3.0)).is_err());
}

#[test]
fn richardson_rejects_nan_input() {
    assert!(richardson(&[f64::NAN, 2.0, 1.0], 2.0).is_err());
}

#[test]
fn schema_rejects_infinite_tolerance() {
    let error = write_case(
        &tmp("inf-tolerance"),
        &body(1, "class = \"N\"\nrel_tol = inf\nabs_tol = 0"),
    )
    .unwrap_err();
    assert!(error.message.contains("rel_tol"));
    assert_eq!(error.line, Some(8));
}

#[test]
fn public_checks_reject_invalid_numbers_and_negative_tolerances() {
    let v = [50.0, 14.0, 5.0];
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..4 {
            let mut args = [1.0, 1.0, 0.0, 0.0];
            args[index] = bad;
            assert!(check_n(args[0], args[1], args[2], args[3]).is_err());
        }
        for index in 0..v.len() {
            let mut values = v;
            values[index] = bad;
            assert!(richardson(&values, 2.0).is_err());
            assert!(check_l(&values, 2.0, 2.0, 0.0, (1.0, 3.0)).is_err());
        }
        assert!(richardson(&[bad, 50.0, 14.0, 5.0], 2.0).is_err());
        assert!(richardson(&v, bad).is_err());
        assert!(check_l(&v, 2.0, bad, 0.0, (1.0, 3.0)).is_err());
        assert!(check_l(&v, 2.0, 2.0, bad, (1.0, 3.0)).is_err());
        assert!(check_l(&v, 2.0, 2.0, 0.0, (bad, 3.0)).is_err());
        assert!(check_l(&v, 2.0, 2.0, 0.0, (1.0, bad)).is_err());
        assert!(check_r(&[bad, 0.5], &[1.0, 0.25], 2.0, 0.0).is_err());
        assert!(check_r(&[1.0, 0.5], &[bad, 0.25], 2.0, 0.0).is_err());
        assert!(check_r(&[1.0, 0.5], &[1.0, 0.25], bad, 0.0).is_err());
        assert!(check_r(&[1.0, 0.5], &[1.0, 0.25], 2.0, bad).is_err());
    }
    assert!(check_n(1.0, 1.0, -1.0, 1.0).is_err());
    assert!(check_n(1.0, 1.0, 1.0, -1.0).is_err());
    assert!(check_l(&v, 2.0, 2.0, -0.1, (1.0, 3.0)).is_err());
    assert!(check_l(&v, 2.0, 2.0, 0.0, (-1.0, 3.0)).is_err());
    assert!(check_l(&v, 2.0, 2.0, 0.0, (3.0, 1.0)).is_err());
    assert!(check_r(&[1.0, 0.5], &[1.0, 0.25], 2.0, -1.0).is_err());
    for ratio in [-1.0, 0.0, 1.0] {
        assert!(richardson(&v, ratio).is_err());
    }
    assert!(check_n(1.0, 1.0, 0.0, 0.0).is_ok());
    assert!(check_r(&[1.0, 0.5], &[1.0, 0.25], 2.0, 0.0).is_ok());
}

#[test]
fn public_checks_reject_non_finite_arithmetic() {
    assert!(check_n(1.0, f64::MAX, 2.0, 0.0).is_err());
    assert!(check_n(f64::MAX, -f64::MAX, 1.0, f64::MAX).is_err());
    assert!(check_l(&[50.0, 14.0, 5.0], 2.0, f64::MAX, 2.0, (1.0, 3.0)).is_err());
    assert!(richardson(&[1.0, 1.0, 1.0], 2.0).is_err());
    assert!(richardson(&[f64::MAX, 0.0, -f64::MAX], 2.0).is_err());
    assert!(richardson(&[1e308, 1.0, 1.0 - f64::EPSILON], 2.0).is_err());
    assert!(richardson(&[f64::MAX, -1e308, -1.1e308], 2.0).is_err());
    assert!(richardson(&[0.0, -1e308, -1.5e308], 2.0).is_err());
}

#[test]
fn schema_rejects_non_finite_numbers_and_invalid_parameters() {
    let dir = tmp("invalid-numbers");
    let classes = [
        (
            1,
            "class = \"N\"\nrel_tol = 0\nabs_tol = 0",
            vec!["rel_tol", "abs_tol"],
        ),
        (
            3,
            "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 1\norder_max = 3",
            vec!["ratio", "reference_error", "order_min", "order_max"],
        ),
        (2, "class = \"R\"\nrate = 2\nband = 0", vec!["rate", "band"]),
    ];
    let mut index = 0;
    for (meshes, parameters, keys) in classes {
        let valid = body(meshes, parameters);
        assert!(write_case(&dir.join(format!("valid{meshes}")), &valid).is_ok());
        for key in keys.iter().copied().chain(std::iter::once("q")) {
            for bad in ["inf", "-inf", "nan", "-1"] {
                if bad == "-1" && ["q", "rate"].contains(&key) {
                    continue;
                }
                let mut lines: Vec<String> = valid.lines().map(str::to_owned).collect();
                let line = lines
                    .iter()
                    .position(|s| s.starts_with(&format!("{key} =")))
                    .unwrap();
                lines[line] = format!("{key} = {bad}");
                index += 1;
                let error =
                    write_case(&dir.join(format!("bad{index}")), &lines.join("\n")).unwrap_err();
                assert!(error.message.contains(key), "{key}: {error}");
                assert_eq!(error.line, Some(line + 1));
            }
        }
        if meshes == 3 {
            for ratio in ["0", "1"] {
                assert!(write_case(
                    &dir.join(format!("ratio{ratio}")),
                    &valid.replace("ratio = 2", &format!("ratio = {ratio}"))
                )
                .is_err());
            }
        }
    }
}
