//! Tests for the pass rule, the fixture schema and the shipped cases.

use fh_validation::{check, load_all, Case, Field, Op};
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
fn body(extra: &str) -> String {
    format!(
        "schema = 3\nid = \"t\"\nunits = \"SI-mm\"\njustification = \"why\"\nderivation = \"d.md\"\nmesh = \"m.inp\"\nrel_tol = 1\nabs_tol = 0\n{extra}\n[ref]\nq = 1.0\n{QUANTITY}"
    )
}

#[test]
fn the_pass_rule_uses_relative_plus_absolute_tolerance() {
    assert!(check(1.0000005, 1.0, 1e-6, 0.0).is_ok());
    assert!(check(1.00001, 1.0, 1e-6, 0.0).is_err());
    assert!(check(1e-12, 0.0, 0.0, 1e-9).is_ok());
}

#[test]
fn schema_reads_a_case() {
    let c = write_case(
        &tmp("ok"),
        &body("").replace("rel_tol = 1\nabs_tol = 0", "rel_tol = 1e-6\nabs_tol = 1e-9"),
    )
    .unwrap();
    assert_eq!((c.rel_tol, c.abs_tol), (1e-6, 1e-9));
    assert_eq!(c.mesh, tmp("ok").join("m.inp"));
    let x = &c.references[0].quantity;
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
fn schema_rejects_bad_fixtures_with_a_reason_and_a_line_when_known() {
    let d = tmp("bad");
    let cases = [
        (body("").replace("abs_tol = 0\n", ""), "abs_tol", None),
        (
            "schema = 3\nid = \"t\"\n[ref]\nq = 1\n".to_string(),
            "justification",
            None,
        ),
        (body("bogus = 1"), "unknown key", Some(9)),
        (body("class = \"N\""), "unknown key `class`", Some(9)),
        (body("element_types = [\"C3D10\"]"), "unknown key `element_types`", Some(9)),
        (body("").replace("m.inp", "nope.inp"), "nope.inp", Some(6)),
        (
            body("").replace("\"m.inp\"", "[\"m.inp\"]"),
            "`mesh` must be a non-empty string",
            Some(6),
        ),
        (
            body("").replace("rel_tol = 1", "rel_tol = \"x\""),
            "rel_tol",
            Some(7),
        ),
        (
            format!("{}\nno equals sign", body("")),
            "case.toml:20",
            Some(20),
        ),
        (
            body("").replace("q = 1.0", "q = \"a\""),
            "reference `q`",
            Some(11),
        ),
        (body("").replace("[ref]\nq = 1.0\n", ""), "[ref]", None),
        (body("").replace("schema = 3\n", ""), "schema", None),
        (
            body("").replace("schema = 3", "schema = 2"),
            "schema version 2",
            Some(1),
        ),
        (
            body("").replace(QUANTITY, ""),
            "no quantity for reference `q`",
            None,
        ),
        (
            format!(
                "{}\n[quantity.w]\nset = \"s\"\nfield = \"U\"\ncomponent = 1\nstep = \"st\"\nop = \"max\"\n",
                body("")
            ),
            "quantity `w` has no reference",
            Some(20),
        ),
        (
            body("").replace("field = \"U\"", "field = \"T\""),
            "unknown field `T`",
            Some(15),
        ),
        (
            body("").replace("component = 1", "component = 4"),
            "component 4 is not valid for field U",
            Some(16),
        ),
        (
            body("")
                .replace("field = \"U\"", "field = \"S\"")
                .replace("component = 1", "component = 3"),
            "component 3 is not valid for field S",
            Some(16),
        ),
        (
            body("").replace("\"mean\"", "\"median\""),
            "unknown op `median`",
            Some(18),
        ),
        (
            body("").replace("set = \"s\"\n", ""),
            "missing key `set`",
            Some(13),
        ),
        (
            body("").replace("set = \"s\"", "set = \"\""),
            "`set` must be a non-empty string",
            Some(14),
        ),
        (
            body("").replace("step = \"st\"", "step = \"\""),
            "`step` must be a non-empty string",
            Some(17),
        ),
        (
            body("").replace("op = \"mean\"", "op = \"mean\"\nbogus = 1"),
            "unknown key `bogus` in `quantity.q`",
            Some(19),
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
fn schema_preserves_string_errors() {
    let dir = tmp("reader-errors");
    let valid = body("");
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
            "cantilever-c3d10",
            "kirsch-plate-c3d10",
            "lame-cylinder-c3d10",
            "plate-on-edge",
            "warpage-strip",
            "warpage-thick-copper"
        ]
    );
    // Every reference of every shipped case says what to measure.
    for c in &cases {
        assert!(!c.justification.is_empty());
        assert!(c.mesh.is_file(), "{}", c.mesh.display());
        assert!(!c.references.is_empty(), "{}", c.id);
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
    let tip = &cases
        .iter()
        .find(|c| c.id == "cantilever-c3d10")
        .unwrap()
        .references[0];
    assert_eq!(tip.value, -0.01919619048);
    let q = &tip.quantity;
    assert_eq!((q.field, q.component, q.op), (Field::U, 3, Op::Mean));
    assert_eq!((q.set.as_str(), q.step.as_str()), ("tip", "tip-load"));
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
fn schema_rejects_infinite_tolerance() {
    let error = write_case(
        &tmp("inf-tolerance"),
        &body("").replace("rel_tol = 1", "rel_tol = inf"),
    )
    .unwrap_err();
    assert!(error.message.contains("rel_tol"));
    assert_eq!(error.line, Some(7));
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
fn schema_rejects_non_finite_numbers_and_invalid_parameters() {
    let dir = tmp("invalid-numbers");
    let valid = body("");
    let mut index = 0;
    for key in ["rel_tol", "abs_tol", "q"] {
        for bad in ["inf", "-inf", "nan", "-1"] {
            if bad == "-1" && key == "q" {
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
}

#[test]
fn units_are_read_and_a_missing_or_unknown_value_is_refused() {
    let case = write_case(&tmp("units-ok"), &body("")).unwrap();
    assert_eq!(case.units, fh_validation::Units::SiMm);
    let metre = body("").replace("units = \"SI-mm\"", "units = \"SI-m\"");
    let case = write_case(&tmp("units-m"), &metre).unwrap();
    assert_eq!(case.units.gmsh_length(), "M");
    let missing = body("").replace("units = \"SI-mm\"\n", "");
    let error = write_case(&tmp("units-missing"), &missing).unwrap_err();
    assert!(error.message.contains("missing key `units`"), "{error}");
    let unknown = body("").replace("SI-mm", "inch");
    let error = write_case(&tmp("units-unknown"), &unknown).unwrap_err();
    assert!(error.message.contains("unknown units `inch`"), "{error}");
    assert_eq!(error.line, Some(3));
}

#[test]
fn the_solver_key_is_read_and_an_unknown_solver_is_refused() {
    let case = write_case(&tmp("solver-default"), &body("")).unwrap();
    assert_eq!(case.solver, "fh-solve-ccx");
    let ccx = body("solver = \"fh-solve-ccx\"");
    let case = write_case(&tmp("solver-ccx"), &ccx).unwrap();
    assert_eq!(case.solver, "fh-solve-ccx");
    let unknown = body("solver = \"abaqus\"");
    let error = write_case(&tmp("solver-unknown"), &unknown).unwrap_err();
    assert_eq!(
        error.message,
        "unknown solver `abaqus`, expected fh-solve-ccx"
    );
    assert_eq!(error.line, Some(9));
}

#[test]
fn the_time_and_memory_limits_are_read_and_a_bad_limit_is_refused() {
    let case = write_case(&tmp("limits-none"), &body("")).unwrap();
    assert_eq!((case.time_limit_s, case.memory_limit_mib), (None, None));
    let limits = body("time_limit_s = 120\nmemory_limit_mib = 0.5");
    let case = write_case(&tmp("limits"), &limits).unwrap();
    assert_eq!(
        (case.time_limit_s, case.memory_limit_mib),
        (Some(120.0), Some(0.5))
    );
    for (bad, why) in [
        ("time_limit_s = 0", "`time_limit_s` must be above 0"),
        (
            "memory_limit_mib = -1",
            "`memory_limit_mib` must be above 0",
        ),
        ("time_limit_s = \"long\"", "`time_limit_s` must be a number"),
    ] {
        let text = body(bad);
        let error = write_case(&tmp("limits-bad"), &text).unwrap_err();
        assert_eq!(error.message, why);
        assert_eq!(error.line, Some(9));
    }
}

const MODEL: &str =
    "\n[model.material]\nname = \"M\"\nyoungs_modulus = 2.0\npoissons_ratio = 0.3\n\
[[model.set]]\nname = \"A\"\naxis = \"z\"\nat = 1.5\n\
[[model.boundary]]\nset = \"A\"\nfirst_dof = 1\nlast_dof = 3\n";

#[test]
fn the_model_table_is_read() {
    let valid = body("");
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
    let valid = body("");
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
    let valid = body("");
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
    let valid = body("");
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
    let valid = body("");
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
    let text = body("").replace("m.inp", "later.inp");
    write_case(&dir, &text).unwrap_err();
    let error = Case::load(&dir).unwrap_err();
    assert!(
        error.message.contains("mesh file later.inp not found"),
        "{error}"
    );
    let case = Case::load_before_mesh(&dir).unwrap();
    assert_eq!(case.mesh, dir.join("later.inp"));
}
