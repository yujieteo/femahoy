//! Snapshot and rule tests for the TOON schemas (`toon-schema.md` section 5).
//! The snapshots start from the examples in the schema. Where the encoder
//! prints a number differently, the snapshot holds the encoder output.

use fh_toon::*;
use serde_json::Value;

fn header(source: &str, hash: &str, units: &str, status: Status) -> Header {
    Header {
        source: source.into(),
        source_hash: hash.into(),
        units: units.into(),
        status,
    }
}

fn diag(code: &str, severity: Severity, line: u32, message: &str) -> Diag {
    Diag {
        code: code.into(),
        severity,
        line,
        message: message.into(),
    }
}

fn next(cmd: &str, why: &str) -> Next {
    Next {
        cmd: cmd.into(),
        why: why.into(),
    }
}

fn rows(n: usize) -> Opts {
    Opts {
        rows: n,
        ..Opts::default()
    }
}

/// The three checks of schema section 5: strict decoding (array length equals
/// rows, rule R7), at most 4 fields per table (R5), and the common blocks (R10 to R12).
fn assert_rules(text: &str) {
    let v: Value = toon_format::decode_strict(text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    let top = v.as_object().expect("top level is an object");
    for key in [
        "schema",
        "version",
        "source",
        "source_hash",
        "units",
        "status",
        "totals",
        "diagnostics",
        "next",
    ] {
        assert!(top.contains_key(key), "missing `{key}`\n{text}");
    }
    assert!(top["next"].as_array().unwrap().len() <= MAX_NEXT);
    for (name, value) in top {
        let Some(table) = value.as_array() else {
            continue;
        };
        for row in table.iter().filter_map(Value::as_object) {
            assert!(
                row.len() <= MAX_FIELDS,
                "table `{name}` has more than 4 fields"
            );
        }
        let more = top
            .get(&format!("{name}_more"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        assert!(
            table.len() as u64 + more > 0 || top.contains_key(&format!("{name}_note")),
            "`{name}` is empty with no note"
        );
    }
}

fn sample_check() -> Check {
    let mut sets = vec![
        SetRow {
            name: "Part-1-1.ALL".into(),
            kind: "elset".into(),
            size: 30112,
            used_by_step: false,
        },
        SetRow {
            name: "Part-1-1.LOAD".into(),
            kind: "surface".into(),
            size: 96,
            used_by_step: true,
        },
        SetRow {
            name: "Part-1-1.FIXED".into(),
            kind: "nset".into(),
            size: 412,
            used_by_step: true,
        },
    ];
    sets.extend((0..11).map(|i| SetRow {
        name: format!("Part-1-1.S{i:02}"),
        kind: "nset".into(),
        size: 1,
        used_by_step: false,
    }));
    let quality = |m: &str, worst, limit| QualityRow {
        metric: m.into(),
        worst,
        limit,
        bad: 0,
    };
    Check {
        header: header("models/bracket.inp", "9f3c1a", "unspecified", Status::Error),
        nodes: 48210,
        element_types: vec![
            ElementType {
                name: "C3D8I".into(),
                count: 312,
            },
            ElementType {
                name: "C3D10".into(),
                count: 29800,
            },
        ],
        sets,
        materials: vec![MaterialRow {
            name: "STEEL".into(),
            behaviour: "elastic-plastic".into(),
            used_by: 1,
        }],
        steps: vec![StepRow {
            name: "Step-1".into(),
            procedure: "static".into(),
            nlgeom: false,
            loads: 2,
        }],
        quality: vec![
            quality("min_scaled_jacobian", 0.21, 0.1),
            quality("max_aspect_ratio", 8.4, 10.0),
            quality("inverted", 0.0, 0.0),
        ],
        diagnostics: vec![
            diag(
                "MSH003",
                Severity::Warning,
                2310,
                "element E1822 is below the quality limit",
            ),
            diag(
                "INP040",
                Severity::Warning,
                88,
                "set Part-1-1.SPARE is never used",
            ),
            diag(
                "INP021",
                Severity::Error,
                1204,
                "unknown keyword *CONTACT CONTROLS",
            ),
        ],
        next: vec![
            next(
                "fh check models/bracket.inp --full",
                "see the fix hint for INP021",
            ),
            next("fh docs keywords", "list supported keywords"),
        ],
    }
}

#[test]
fn check_snapshot() {
    let text = sample_check().render(&rows(3)).unwrap();
    assert_eq!(text, include_str!("snapshots/check.toon"));
    assert_rules(&text);
}

fn case(id: &str, class: &str, status: CaseStatus, margin: f64) -> CaseRow {
    CaseRow {
        id: id.into(),
        class: class.into(),
        status,
        margin,
    }
}

#[test]
fn validate_snapshot() {
    let mut cases = vec![
        case("lame-cylinder", "L", CaseStatus::Pass, 0.81),
        case("cantilever-c3d8i", "L", CaseStatus::Pass, 0.42),
        case("hertz-sphere", "N", CaseStatus::Fail, 1.37),
    ];
    cases.extend((0..10).map(|i| case(&format!("c{i}"), "L", CaseStatus::Pass, 0.1)));
    let v = Validate {
        header: header("validation/cases", "c01d9e", "unspecified", Status::Failed),
        cases,
        diagnostics: vec![],
        next: vec![next("fh validate hertz-sphere", "see which quantity fails")],
    };
    let text = v.render(&rows(3)).unwrap();
    assert_eq!(text, include_str!("snapshots/validate.toon"));
    assert_rules(&text);
}

fn quantity(name: &str, value: f64, reference: f64, margin: f64) -> Quantity {
    Quantity {
        name: name.into(),
        value,
        reference,
        margin,
    }
}

fn sample_case() -> ValidateCase {
    ValidateCase {
        header: header(
            "validation/cases/hertz-sphere",
            "5e2f10",
            "SI-mm",
            Status::Failed,
        ),
        quantities: vec![
            quantity("peak_pressure", 1688.0, 1592.0, 1.37),
            quantity("contact_radius", 0.612, 0.604, 0.66),
            quantity("total_force", 1000.0, 1000.0, 0.0),
        ],
        refinement: None,
        rates: None,
        tolerance: None,
        diagnostics: vec![],
        next: vec![next(
            "fh validate hertz-sphere --full",
            "see tolerances and mesh data",
        )],
    }
}

#[test]
fn validate_case_snapshot() {
    let text = sample_case().render(&Opts::default()).unwrap();
    assert_eq!(text, include_str!("snapshots/validate_case.toon"));
    assert_rules(&text);
}

#[test]
fn validate_case_extra_tables() {
    let mut c = sample_case();
    c.refinement = Some(vec![Refinement {
        mesh: "coarse".into(),
        size: 0.5,
        value: 1650.0,
    }]);
    c.rates = Some(vec![Rate {
        field: "U.MAG".into(),
        observed: 1.98,
        expected: 2.0,
    }]);
    c.tolerance = Some(vec![Tolerance {
        name: "peak_pressure".into(),
        tolerance: 0.05,
        source: "hertz".into(),
    }]);
    let text = c.render(&Opts::default()).unwrap();
    assert!(text.contains("refinement[1]{mesh,size,value}:\n  coarse,0.5,1650\n"));
    assert!(text.contains("rates[1]{field,observed,expected}:"));
    assert!(text.contains("tolerance[1]{name,tolerance,source}:"));
    assert_rules(&text);
}

fn inc(n: u64, iters: u64, result: IncResult) -> Increment {
    Increment {
        step: "Step-1".into(),
        inc: n,
        iters,
        result,
    }
}

fn sample_run() -> Run {
    let results = [
        (1, 4, IncResult::Ok),
        (2, 4, IncResult::Ok),
        (3, 4, IncResult::Ok),
        (4, 12, IncResult::Cutback),
        (5, 3, IncResult::Ok),
        (6, 3, IncResult::Ok),
        (7, 4, IncResult::Ok),
        (8, 4, IncResult::Ok),
        (9, 3, IncResult::Ok),
    ];
    Run {
        header: header("models/bracket.inp", "9f3c1a", "unspecified", Status::Ok),
        run_id: "r-0007".into(),
        solver: "core".into(),
        results: "results/bracket.pvd".into(),
        wall_s: 38.2,
        steps: vec![RunStep {
            name: "Step-1".into(),
            status: "converged".into(),
            reached: 1.0,
            increments: 9,
        }],
        increments: results.iter().map(|&(n, i, r)| inc(n, i, r)).collect(),
        contact: Some(vec![ContactRow {
            pair: "CP-1".into(),
            closed: 214,
            max_pen: 2.1e-4,
            force: 1520.0,
        }]),
        failure: None,
        diagnostics: vec![],
        next: vec![next("fh results", "see extremes and reactions")],
    }
}

#[test]
fn run_snapshot() {
    let text = sample_run().render(&rows(2)).unwrap();
    assert_eq!(text, include_str!("snapshots/run.toon"));
    assert_rules(&text);
}

#[test]
fn run_failed_snapshot() {
    let residual = |iter, force, disp, contact_changes| Residual {
        iter,
        force,
        disp,
        contact_changes,
    };
    let worst = |label: &str, force| Worst {
        label: label.into(),
        set: "Part-1-1.CONTACT_TOP".into(),
        force,
    };
    let mut r = sample_run();
    r.header = header(
        "models/bracket.inp",
        "9f3c1a",
        "unspecified",
        Status::Failed,
    );
    r.run_id = "r-0008".into();
    r.wall_s = 12.5;
    r.steps = vec![RunStep {
        name: "Step-1".into(),
        status: "failed".into(),
        reached: 0.25,
        increments: 1,
    }];
    r.increments = vec![inc(1, 4, IncResult::Failed)];
    r.contact = None;
    r.failure = Some(Failure {
        residuals: vec![
            residual(1, 320.0, 1.0, 14),
            residual(2, 81.0, 0.42, 9),
            residual(3, 79.0, 0.4, 11),
            residual(4, 83.0, 0.44, 10),
        ],
        worst: vec![
            worst("N2210", 38.9),
            worst("N2207", 41.2),
            worst("N1", 3.0),
            worst("N2", 2.0),
            worst("N3", 1.0),
        ],
    });
    r.diagnostics = vec![diag(
        "SOL001",
        Severity::Error,
        0,
        "increment 1 did not converge",
    )];
    r.next = vec![next(
        "fh run models/bracket.inp --force",
        "try a smaller first increment",
    )];
    let text = r.render(&rows(2)).unwrap();
    assert_eq!(text, include_str!("snapshots/run_failed.toon"));
    assert_rules(&text);
}

#[test]
fn running_run_keeps_the_common_blocks() {
    let mut r = sample_run();
    r.header.status = Status::Running;
    r.next = vec![next("fh wait r-0007", "wait for the solve")];
    assert_rules(&r.render(&Opts::default()).unwrap());
}

#[test]
fn empty_table_prints_zero_length_and_note() {
    let mut c = sample_check();
    c.materials.clear();
    let text = c.render(&Opts::default()).unwrap();
    assert!(
        text.contains("materials[0]:\nmaterials_note: no materials\n"),
        "{text}"
    );
    assert_rules(&text);
}

#[test]
fn row_limit_ranks_and_counts_the_rest() {
    let text = sample_check().render(&rows(1)).unwrap();
    assert!(
        text.contains("element_types[1]{type,count}:\n  C3D10,29800\nelement_types_more: 1\n"),
        "{text}"
    );
    assert!(
        text.contains("diagnostics[1]{code,severity,line,message}:\n  INP021,error,1204,"),
        "{text}"
    );
    assert!(text.contains("diagnostics_more: 2\n"), "{text}");
    assert_rules(&text);
}

#[test]
fn zero_row_limit_reports_the_rows_it_hid() {
    let text = sample_check().render(&rows(0)).unwrap();
    assert!(
        text.contains("element_types[0]:\nelement_types_more: 2\n"),
        "{text}"
    );
    assert!(!text.contains("element_types_note"), "{text}");
}

#[test]
fn next_has_at_most_three_rows() {
    let mut c = sample_check();
    c.next = (0..5)
        .map(|i| next(&format!("fh step{i}"), "why"))
        .collect();
    let text = c.render(&Opts::default()).unwrap();
    assert!(text.contains("next[3]{cmd,why}:"), "{text}");
    assert!(!text.contains("next_more"));
    assert_rules(&text);
}

#[test]
fn precision_sets_significant_figures() {
    let mut r = sample_run();
    r.wall_s = 123.456789;
    let default = r.render(&Opts::default()).unwrap();
    assert!(default.contains("wall_s: 123.5\n"), "{default}");
    let long = r
        .render(&Opts {
            precision: 7,
            ..Opts::default()
        })
        .unwrap();
    assert!(long.contains("wall_s: 123.4568\n"), "{long}");
}

#[test]
fn tab_delimiter_decodes_with_the_same_rules() {
    let text = sample_check()
        .render(&Opts {
            delim: Delim::Tab,
            ..Opts::default()
        })
        .unwrap();
    assert!(text.contains('\t'));
    let opts = toon_format::DecodeOptions::new().with_strict(true);
    let v: Value = toon_format::decode(&text, &opts).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(v["sets"].as_array().unwrap().len(), 14);
}

#[test]
fn exit_codes_match_the_schema() {
    let codes = [
        ExitCode::Success,
        ExitCode::Failure,
        ExitCode::Usage,
        ExitCode::Internal,
    ]
    .map(ExitCode::code);
    assert_eq!(codes, [0, 1, 2, 3]);
}

#[test]
fn pass_margin_never_prints_as_one() {
    let mut c = sample_case();
    c.quantities = vec![
        quantity("a", 1.0, 1.0, 0.99996),
        quantity("b", 1.0, 1.0, 1.00004),
        quantity("c", 1.0, 1.0, 0.5),
    ];
    let text = c.render(&Opts::default()).unwrap();
    assert!(text.contains("  a,1,1,0.9999\n"), "{text}");
    assert!(text.contains("  b,1,1,1\n"), "{text}");
    assert!(text.contains("  c,1,1,0.5\n"), "{text}");
    let v = Validate {
        header: header("validation/cases", "c01d9e", "unspecified", Status::Ok),
        cases: vec![case("near", "L", CaseStatus::Pass, 0.99996)],
        diagnostics: vec![],
        next: vec![],
    };
    assert!(v
        .render(&Opts::default())
        .unwrap()
        .contains("  near,L,pass,0.9999\n"));
}
