//! Tests for the tolerance classes, the fixture schema and the shipped cases.

use fh_validation::{
    check_l, check_n, check_r, load_all, richardson, run_convergence_with, Case, Class, Field, Op,
    Outcome, Solve,
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
        "schema = 2\nid = \"t\"\nunits = \"SI-mm\"\njustification = \"why\"\nderivation = \"d.md\"\nmesh = [{list}]\nelement_types = [\"C3D8\"]\n{extra}\n[ref]\nq = 1.0\n{QUANTITY}"
    )
}

/// A class R fixture body: no `[ref]`, the manufactured solution instead.
fn body_r(meshes: usize, extra: &str) -> String {
    let list = vec!["\"m.inp\""; meshes].join(", ");
    let sizes: Vec<String> = (0..meshes)
        .map(|i| format!("{}", 1.0 / (1 << i) as f64))
        .collect();
    format!(
        "schema = 2\nid = \"t\"\nunits = \"SI-mm\"\njustification = \"why\"\nderivation = \"d.md\"\nmesh = [{list}]\nelement_types = [\"C3D8\"]\nclass = \"R\"\nmesh_size = [{}]\nboundary_set = \"B\"\n{extra}\n[exact]\nu1 = \"x\"\nu2 = \"2 * y\"\nu3 = \"sin(z)\"\n",
        sizes.join(", ")
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
fn richardson_does_not_depend_on_the_scale_of_the_values() {
    // f(h) = 3 h^2 with h = 4, 2, 1, scaled across the finite range. The values are
    // monotone, also where the product of two small differences underflows to 0.
    let mut bad = Vec::new();
    for e in (-300..=300).step_by(25) {
        for sign in [1.0, -1.0] {
            let s: f64 = sign * 10f64.powi(e);
            match richardson(&[48.0 * s, 12.0 * s, 3.0 * s], 2.0) {
                Ok((limit, order, _))
                    if (order - 2.0).abs() < 1e-9 && limit.abs() <= 1e-9 * s.abs() => {}
                r => bad.push(format!("scale {s:e}: {r:?}")),
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
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
        // The limit sits close to the reference and the order is near 2 for these elements.
        assert!(
            (limit - r).abs() <= 0.02 * r.abs(),
            "{id} {name}: limit {limit}, ref {r}"
        );
        assert!((1.5..=2.5).contains(&order), "{id} {name}: order {order}");
        // A wrong solver result must fail: a 10 percent error in the finest value, or in the
        // reference, is outside the tolerance.
        let mut wrong = values;
        wrong[3] *= 1.1;
        assert!(
            check_l(&wrong, ratio, r, e, band).is_err(),
            "{id} {name}: wrong value"
        );
        assert!(
            check_l(&values, ratio, r * 1.1, e, band).is_err(),
            "{id} {name}: wrong ref"
        );
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
    let r = write_case(&d.join("r"), &body_r(2, "rate = 2\nband = 0.2")).unwrap();
    assert!(matches!(r.class, Class::R { .. }));
    assert!(r.references.is_empty());
    assert_eq!(r.element_types, ["C3D8"]);
    let m = r.manufactured.as_ref().unwrap();
    assert_eq!(
        (m.mesh_sizes.as_slice(), m.boundary_set.as_str()),
        (&[1.0, 0.5][..], "B")
    );
    assert_eq!(m.exact[1].eval([0.0, 3.0, 0.0]), 6.0);
    assert!(l.manufactured.is_none() && n.manufactured.is_none());
    let x = &l.references[0].quantity;
    assert_eq!((x.field, x.component, x.set.as_str()), (Field::U, 1, "s"));
    assert_eq!((x.step.as_str(), x.op), ("st", Op::Mean));
}

#[test]
fn class_r_rejects_a_bad_manufactured_solution() {
    let d = tmp("r-bad");
    let ok = body_r(2, "rate = 2\nband = 0.2");
    assert!(write_case(&d.join("ok"), &ok).is_ok());
    let cases = [
        ("mesh_size = [1, 0.5]", "mesh_size = [1]", "needs 2 values"),
        ("mesh_size = [1, 0.5]", "mesh_size = [0.5, 1]", "falling"),
        ("mesh_size = [1, 0.5]", "mesh_size = [1, -0.5]", "positive"),
        ("mesh_size = [1, 0.5]\n", "", "missing key `mesh_size`"),
        ("boundary_set = \"B\"\n", "", "missing key `boundary_set`"),
        ("u2 = \"2 * y\"", "u2 = \"2 * w\"", "unknown name `w`"),
        ("u2 = \"2 * y\"", "u2 = \"2 *\"", "`exact.u2`"),
        ("u3 = \"sin(z)\"\n", "", "missing key `u3`"),
        (
            "u3 = \"sin(z)\"",
            "u3 = \"z\"\nu4 = \"z\"",
            "unknown key `u4`",
        ),
        (
            "rate = 2",
            "rate = 2\nref = 1",
            "`ref` does not belong to class R",
        ),
        (
            "class = \"R\"",
            "class = \"N\"\nrel_tol = 0\nabs_tol = 0",
            "does not belong to class N",
        ),
    ];
    for (i, (old, new, message)) in cases.iter().enumerate() {
        let text = ok.replace(old, new);
        assert_ne!(text, ok, "{old}");
        let e = write_case(&d.join(format!("c{i}")), &text).unwrap_err();
        assert!(e.message.contains(message), "{message}: {e}");
    }
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
            Some(6),
        ),
        (body(2, n), "exactly 1", Some(6)),
        (body(1, "class = \"Z\""), "unknown class", Some(8)),
        (body(1, "class = \"N\""), "rel_tol", None),
        (
            "schema = 2\nid = \"t\"\nclass = \"N\"\n[ref]\nq = 1\n".to_string(),
            "justification",
            None,
        ),
        (body(1, &format!("{n}\nbogus = 1")), "unknown key", Some(11)),
        (
            body(1, &format!("{n}\nratio = 2")),
            "`ratio` does not belong to class N",
            Some(11),
        ),
        (
            body(
                3,
                "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 1\norder_max = 3\nrel_tol = 1",
            ),
            "`rel_tol` does not belong to class L",
            Some(13),
        ),
        (body(1, n).replace("m.inp", "nope.inp"), "nope.inp", Some(6)),
        (
            body(1, "class = \"N\"\nrel_tol = \"x\"\nabs_tol = 0"),
            "rel_tol",
            Some(9),
        ),
        (
            format!("{}\nno equals sign", body(1, n)),
            "case.toml:21",
            Some(21),
        ),
        (
            body(1, n).replace("q = 1.0", "q = \"a\""),
            "reference `q`",
            Some(12),
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
            Some(21),
        ),
        (
            body(1, n).replace("field = \"U\"", "field = \"T\""),
            "unknown field `T`",
            Some(16),
        ),
        (
            body(1, n).replace("component = 1", "component = 4"),
            "component 4 is not valid for field U",
            Some(17),
        ),
        (
            body(1, n)
                .replace("field = \"U\"", "field = \"S\"")
                .replace("component = 1", "component = 3"),
            "component 3 is not valid for field S",
            Some(17),
        ),
        (
            body(1, n).replace("\"mean\"", "\"median\""),
            "unknown op `median`",
            Some(19),
        ),
        (
            body(1, n).replace("set = \"s\"\n", ""),
            "missing key `set`",
            Some(14),
        ),
        (
            body(1, n).replace("set = \"s\"", "set = \"\""),
            "`set` must be a non-empty string",
            Some(15),
        ),
        (
            body(1, n).replace("step = \"st\"", "step = \"\""),
            "`step` must be a non-empty string",
            Some(18),
        ),
        (
            body(1, n).replace("op = \"mean\"", "op = \"mean\"\nbogus = 1"),
            "unknown key `bogus` in `quantity.q`",
            Some(20),
        ),
        (
            body(1, n).replace("element_types = [\"C3D8\"]\n", ""),
            "element_types",
            None,
        ),
        (
            body(1, n).replace("[\"C3D8\"]", "[]"),
            "`element_types` must be a non-empty array",
            Some(7),
        ),
        (
            body(1, n).replace("\"C3D8\"", "\"S4R\""),
            "unknown element type `S4R`",
            Some(7),
        ),
        (
            body(3, "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 3\norder_max = 1"),
            "order_min must be below order_max",
            Some(11),
        ),
        (
            body(3, "class = \"L\"\nratio = 2\nreference_error = 0\norder_min = 0\norder_max = 1"),
            "order_min must be above 0",
            Some(11),
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
            Some(15),
        ),
        (
            "field = \"U\"",
            "field = \"\"",
            "`field` must be a non-empty string",
            Some(16),
        ),
        (
            "op = \"mean\"",
            "op = false",
            "`op` must be a non-empty string",
            Some(19),
        ),
        (
            "class = \"N\"",
            "class = \"\"",
            "`class` must be a non-empty string",
            Some(8),
        ),
        (
            "class = \"N\"",
            "class = \"Z\"",
            "unknown class `Z`, expected L, N or R",
            Some(8),
        ),
        (
            "class = \"N\"",
            "class = \"L\"",
            "`rel_tol` does not belong to class L",
            Some(9),
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
    assert_eq!(
        ids,
        [
            "bar-pressure",
            "bar-pressure-m",
            "bar-tension",
            "bimetal-strip",
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
            "warpage-strip"
        ]
    );
    for c in &cases {
        // Class R measures the whole field, so it has no point references.
        let is_r = matches!(c.class, Class::R { .. });
        assert_eq!(c.references.is_empty(), is_r, "{}", c.id);
        assert_eq!(c.manufactured.is_some(), is_r, "{}", c.id);
        assert!(!c.justification.is_empty());
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
    for (base, variant, ty) in [
        ("cantilever", "cantilever-c3d10", "C3D10"),
        ("cantilever", "cantilever-c3d8", "C3D8"),
        ("lame-cylinder", "lame-cylinder-c3d10", "C3D10"),
        ("lame-cylinder", "lame-cylinder-c3d8i", "C3D8I"),
        ("patch-test", "patch-test-c3d10", "C3D10"),
        ("patch-test", "patch-test-c3d8i", "C3D8I"),
    ] {
        let original = cases.iter().find(|c| c.id == base).unwrap();
        let added = cases.iter().find(|c| c.id == variant).unwrap();
        assert_eq!(types(variant), [ty]);
        assert_eq!(added.class, original.class, "{variant} tolerances");
        assert_eq!(added.units, original.units, "{variant} units");
        assert_eq!(
            added.meshes.len(),
            original.meshes.len(),
            "{variant} meshes"
        );
        assert_eq!(added.references.len(), original.references.len());
        for (a, b) in added.references.iter().zip(&original.references) {
            assert_eq!(a.name, b.name, "{variant}");
            assert_eq!(a.value, b.value, "{variant} {}", a.name);
            assert_eq!(a.quantity, b.quantity, "{variant} {}", a.name);
        }
    }

    // One class R case for each element type.
    assert_eq!(types("mms-c3d8"), ["C3D8"]);
    assert_eq!(types("mms-c3d8i"), ["C3D8I"]);
    assert_eq!(types("mms-c3d10"), ["C3D10"]);
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
fn convergence_needs_a_mesh_sequence() {
    struct Unused;
    impl Solve for Unused {
        fn measure(&self, _: &Case, _: &std::path::Path) -> Result<Vec<f64>, String> {
            Err("not called".into())
        }
    }
    for c in load_all(&root()).unwrap() {
        if let Class::N { .. } = c.class {
            let out = run_convergence_with(&c, &Unused).outcome;
            assert!(
                matches!(out, Outcome::Fail(ref m) if m.contains("class N")),
                "{}",
                c.id
            );
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
    assert_eq!(error.line, Some(9));
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
        (2, "rate = 2\nband = 0", vec!["rate", "band"]),
    ];
    let mut index = 0;
    for (meshes, parameters, keys) in classes {
        let is_r = parameters.starts_with("rate");
        let valid = if is_r {
            body_r(meshes, parameters)
        } else {
            body(meshes, parameters)
        };
        assert!(write_case(&dir.join(format!("valid{meshes}")), &valid).is_ok());
        // Class R has no reference value `q`.
        for key in keys.iter().copied().chain((!is_r).then_some("q")) {
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

#[test]
fn units_are_read_and_a_missing_or_unknown_value_is_refused() {
    let n = "class = \"N\"\nrel_tol = 1\nabs_tol = 0";
    let case = write_case(&tmp("units-ok"), &body(1, n)).unwrap();
    assert_eq!(case.units, fh_validation::Units::SiMm);
    let metre = body(1, n).replace("units = \"SI-mm\"", "units = \"SI-m\"");
    let case = write_case(&tmp("units-m"), &metre).unwrap();
    assert_eq!(case.units.gmsh_length(), "M");
    let missing = body(1, n).replace("units = \"SI-mm\"\n", "");
    let error = write_case(&tmp("units-missing"), &missing).unwrap_err();
    assert!(error.message.contains("missing key `units`"), "{error}");
    let unknown = body(1, n).replace("SI-mm", "inch");
    let error = write_case(&tmp("units-unknown"), &unknown).unwrap_err();
    assert!(error.message.contains("unknown units `inch`"), "{error}");
    assert_eq!(error.line, Some(3));
}

#[test]
fn the_solver_key_is_read_and_an_unknown_solver_is_refused() {
    let n = "class = \"N\"\nrel_tol = 1\nabs_tol = 0";
    let case = write_case(&tmp("solver-default"), &body(1, n)).unwrap();
    assert_eq!(case.solver, "fh-solve");
    let ccx = body(1, &format!("{n}\nsolver = \"fh-solve-ccx\""));
    let case = write_case(&tmp("solver-ccx"), &ccx).unwrap();
    assert_eq!(case.solver, "fh-solve-ccx");
    let unknown = body(1, &format!("{n}\nsolver = \"abaqus\""));
    let error = write_case(&tmp("solver-unknown"), &unknown).unwrap_err();
    assert_eq!(
        error.message,
        "unknown solver `abaqus`, expected fh-solve or fh-solve-ccx"
    );
    assert_eq!(error.line, Some(11));
}

const MODEL: &str =
    "\n[model.material]\nname = \"M\"\nyoungs_modulus = 2.0\npoissons_ratio = 0.3\n\
[[model.set]]\nname = \"A\"\naxis = \"z\"\nat = 1.5\n\
[[model.boundary]]\nset = \"A\"\nfirst_dof = 1\nlast_dof = 3\n";

#[test]
fn the_model_table_is_read() {
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let case = write_case(&tmp("model-ok"), &format!("{valid}{MODEL}")).unwrap();
    let model = case.model.unwrap();
    assert_eq!(model.step, "LOAD");
    assert_eq!(
        (model.sets[0].axis, model.sets[0].at, model.sets[0].tol),
        (2, 1.5, None)
    );
    assert_eq!(model.boundaries[0].value, 0.0);
    assert!(write_case(&tmp("model-none"), &valid)
        .unwrap()
        .model
        .is_none());
}

#[test]
fn thermal_materials_and_a_point_support_are_checked() {
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let thermal = format!(
        "{valid}\n[model]\ntemperature_change = -100\n{}",
        MODEL
            .replace("[model.material]", "[[model.material]]")
            .replace(
                "name = \"M\"",
                "name = \"M\"\nexpansion = 1e-5\nbox = [0, 0, 0, 1, 1, 1]"
            )
            .replace("axis = \"z\"\nat = 1.5", "point = [0, 0, 0]")
    );
    let case = write_case(&tmp("thermal-ok"), &thermal).unwrap();
    let model = case.model.unwrap();
    assert_eq!(model.temperature_change, Some(-100.0));
    assert_eq!(model.materials[0].expansion, Some(1e-5));
    assert_eq!(model.sets[0].point, Some([0.0; 3]));
    for (from, to, message) in [
        ("expansion = 1e-5", "", "needs expansion"),
        (
            "box = [0, 0, 0, 1, 1, 1]",
            "box = [0, 0, 0, 0, 1, 1]",
            "positive extents",
        ),
        (
            "point = [0, 0, 0]",
            "point = [0, 0]",
            "3 finite coordinates",
        ),
        (
            "point = [0, 0, 0]",
            "point = [0, 0, 0]\naxis = \"x\"",
            "point cannot use",
        ),
        (
            "name = \"M\"",
            "name = \"M;Exit\"",
            "material name is invalid",
        ),
    ] {
        let error = write_case(&tmp("thermal-bad"), &thermal.replace(from, to)).unwrap_err();
        assert!(error.message.contains(message), "{error}");
    }
}

#[test]
fn a_bad_model_table_is_an_error() {
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let cases = [
        (
            "set = \"A\"",
            "set = \"B\"",
            "`B` is not a `[[model.set]]` or `[[model.region]]` name",
        ),
        ("axis = \"z\"", "axis = \"w\"", "`axis` must be x, y or z"),
        (
            "name = \"A\"",
            "name = \"all\"",
            "set name `all` is reserved or repeated",
        ),
        (
            "last_dof = 3",
            "last_dof = 4",
            "`last_dof` must be 1, 2 or 3",
        ),
        (
            "poissons_ratio = 0.3",
            "poissons_ratio = 0.5",
            "`poissons_ratio` must be above -1 and below 0.5",
        ),
        (
            "youngs_modulus = 2.0",
            "youngs_modulus = 2.0\nextra = 1",
            "unknown key `extra` in `model.material`",
        ),
    ];
    for (index, (old, new, message)) in cases.iter().enumerate() {
        let text = format!("{valid}{}", MODEL.replace(old, new));
        let error = write_case(&tmp(&format!("model-bad{index}")), &text).unwrap_err();
        assert_eq!(error.message, *message);
    }
    let no_boundary = MODEL.split("[[model.boundary]]").next().unwrap();
    let error = write_case(&tmp("model-nobc"), &format!("{valid}{no_boundary}")).unwrap_err();
    assert!(
        error.message.contains("at least one `[[model.boundary]]`"),
        "{}",
        error.message
    );
}

const REGIONS: &str = "[[model.region]]\nname = \"LOAD\"\naxis = \"z\"\nat = 9.0\n\
[[model.region]]\nname = \"BORE\"\nbox = [0, 0, 0, 1, 1, 2.5]\n\
[[model.boundary]]\nset = \"BORE\"\nfirst_dof = 3\n\
[[model.dsload]]\nsurface = \"LOAD\"\npressure = -4.5\n";

#[test]
fn regions_and_pressure_loads_are_read() {
    use fh_validation::Select;
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let case = write_case(&tmp("regions-ok"), &format!("{valid}{MODEL}{REGIONS}")).unwrap();
    let model = case.model.unwrap();
    assert_eq!(model.regions.len(), 2);
    assert_eq!(model.regions[0].name, "LOAD");
    assert_eq!(model.regions[0].select, Select::Plane { axis: 2, at: 9.0 });
    assert_eq!(
        model.regions[1].select,
        Select::Box {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 1.0, 2.5]
        }
    );
    assert_eq!(model.boundaries[1].set, "BORE");
    assert_eq!(model.dsloads[0].surface, "LOAD");
    assert_eq!(model.dsloads[0].pressure, -4.5);
}

#[test]
fn a_bad_region_or_pressure_load_is_an_error() {
    let valid = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0");
    let cases = [
        (
            "surface = \"LOAD\"",
            "surface = \"A\"",
            "`A` is not a `[[model.region]]` name",
        ),
        (
            "name = \"LOAD\"",
            "name = \"A\"",
            "region name `A` is reserved or repeated",
        ),
        (
            "name = \"BORE\"",
            "name = \"solid\"",
            "region name `solid` is reserved or repeated",
        ),
        (
            "name = \"BORE\"",
            "name = \"2nd face\"",
            "region name `2nd face` must start with a letter and hold only letters, digits and _",
        ),
        (
            "axis = \"z\"\nat = 9.0",
            "axis = \"z\"\nat = 9.0\nbox = [0, 0, 0, 1, 1, 1]",
            "a region needs `axis` and `at`, or `box`, not both",
        ),
        (
            "box = [0, 0, 0, 1, 1, 2.5]",
            "box = [0, 0, 0, 1, 1]",
            "`box` must be 6 numbers: x0, y0, z0, x1, y1, z1",
        ),
        (
            "box = [0, 0, 0, 1, 1, 2.5]",
            "box = [0, 0, 3, 1, 1, 2.5]",
            "`box` needs x0 <= x1, y0 <= y1 and z0 <= z1",
        ),
        (
            "pressure = -4.5",
            "pressure = -4.5\nextra = 1",
            "unknown key `extra` in `model.dsload`",
        ),
    ];
    for (index, (old, new, message)) in cases.iter().enumerate() {
        let text = format!("{valid}{MODEL}{}", REGIONS.replace(old, new));
        let error = write_case(&tmp(&format!("region-bad{index}")), &text).unwrap_err();
        assert_eq!(error.message, *message, "case {index}");
    }
}

#[test]
fn a_case_before_its_mesh_loads_only_with_load_before_mesh() {
    let dir = tmp("before-mesh");
    let text = body(1, "class = \"N\"\nrel_tol = 1\nabs_tol = 0").replace("m.inp", "later.inp");
    write_case(&dir, &text).unwrap_err();
    let error = Case::load(&dir).unwrap_err();
    assert!(
        error.message.contains("mesh file later.inp not found"),
        "{error}"
    );
    let case = Case::load_before_mesh(&dir).unwrap();
    assert_eq!(case.meshes, [dir.join("later.inp")]);
}
