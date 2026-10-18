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
        "precision",
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

#[test]
fn unsigned_cells_preserve_large_values() {
    for value in [i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        let cell = Cell::from(value);
        let table = Table::new("items", ["count"], "no items").ranked([[cell.clone()]], 1);
        let text = Doc::new()
            .scalar("count", cell.clone())
            .block("totals", vec![("count", cell)])
            .table(table)
            .encode(&Opts::default())
            .unwrap();
        assert_eq!(
            text,
            format!("count: {value}\ntotals:\n  count: {value}\nitems[1]{{count}}:\n  {value}\n")
        );
    }
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
fn run_total_overflow_returns_an_error() {
    let mut r = sample_run();
    r.increments = vec![inc(1, u64::MAX, IncResult::Ok), inc(2, 1, IncResult::Ok)];
    assert_eq!(
        r.render(&rows(0)).unwrap_err().to_string(),
        "toon encode failed: iteration count overflow"
    );
}

#[test]
fn totals_preserve_the_largest_valid_sum() {
    let mut r = sample_run();
    r.increments = vec![
        inc(1, u64::MAX - 1, IncResult::Ok),
        inc(2, 1, IncResult::Ok),
    ];
    assert!(r
        .render(&rows(0))
        .unwrap()
        .contains(&format!("  iterations: {}\n", u64::MAX)));
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
    let mut r = sample_run();
    r.steps.clear();
    let text = r.render(&Opts::default()).unwrap();
    assert!(text.contains("steps[0]:\nsteps_note: no steps\n"), "{text}");
    assert_rules(&text);
}

#[test]
fn row_limit_ranks_and_counts_the_rest() {
    let mut r = sample_run();
    r.diagnostics = vec![
        diag("SOL101", Severity::Warning, 30, "a"),
        diag("SOL001", Severity::Error, 40, "b"),
        diag("SOL101", Severity::Warning, 10, "c"),
    ];
    let text = r.render(&rows(1)).unwrap();
    assert!(
        text.contains(
            "increments[1]{step,inc,iters,result}:\n  Step-1,4,12,cutback\nincrements_more: 8\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("diagnostics[1]{code,severity,line,message}:\n  SOL001,error,40,b\n"),
        "{text}"
    );
    assert!(text.contains("diagnostics_more: 2\n"), "{text}");
    assert_rules(&text);
}

#[test]
fn zero_row_limit_reports_the_rows_it_hid() {
    let text = sample_run().render(&rows(0)).unwrap();
    assert!(
        text.contains("increments[0]:\nincrements_more: 9\n"),
        "{text}"
    );
    assert!(!text.contains("increments_note"), "{text}");
}

#[test]
fn ranked_replaces_rows_and_hidden_count() {
    for limit in [0, 1, 3, usize::MAX] {
        let table = Table::new("items", ["value"], "no items")
            .ranked([["old".into()]; 1], 0)
            .ranked((0..3u32).map(|n| [n.into()]), limit);
        let text = Doc::new().table(table).encode(&Opts::default()).unwrap();
        let decoded: Value = toon_format::decode_strict(&text).unwrap();
        assert_eq!(decoded["items"].as_array().unwrap().len(), limit.min(3));
        if limit < 3 {
            assert_eq!(decoded["items_more"], 3 - limit);
            assert!(decoded.get("items_note").is_none());
        } else {
            assert!(decoded.get("items_more").is_none());
        }
    }
    let table = Table::new("items", ["value"], "no items")
        .ranked([["old".into()]; 1], 0)
        .ranked([], 1);
    assert_eq!(
        Doc::new().table(table).encode(&Opts::default()).unwrap(),
        "items[0]:\nitems_note: no items\n"
    );
}

#[test]
fn ranked_stops_at_the_first_end_of_the_iterator() {
    let mut n = 0u32;
    let rows = std::iter::from_fn(move || {
        n += 1;
        (n != 2).then(|| [n.into()])
    });
    let table = Table::new("items", ["value"], "no items").ranked(rows, 3);
    assert_eq!(
        Doc::new().table(table).encode(&Opts::default()).unwrap(),
        "items[1]{value}:\n  1\n"
    );
}

#[test]
fn next_has_at_most_three_rows() {
    let mut r = sample_run();
    r.next = (0..5)
        .map(|i| next(&format!("fh step{i}"), "why"))
        .collect();
    let text = r.render(&Opts::default()).unwrap();
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
fn header_states_precision_and_version() {
    let r = sample_run();
    let default = r.render(&Opts::default()).unwrap();
    assert!(default.contains("version: 1.1.0\n"), "{default}");
    assert!(default.contains("status: ok\nprecision: 4\n"), "{default}");
    let long = r
        .render(&Opts {
            precision: 7,
            ..Opts::default()
        })
        .unwrap();
    assert!(long.contains("\nprecision: 7\n"), "{long}");
}
