//! `fh validate [--root DIR]`: load every case and run the harness.
//!
//! Each case runs through `ccx` (`$FH_CCX`, else `ccx` on PATH). A table shows every quantity
//! with its computed value, reference, tolerance and margin. A bad fixture, a case that fails
//! or a case that cannot run gives exit code 1. If `ccx` does not start, `fh validate` stops
//! before the first case and tells how to install CalculiX.

use crate::measure::Subprocess;
use fh_solver_ccx::Ccx;
use fh_validation::{load_all, run_case_with, Case, Field, Outcome, Report};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

/// Prints the error and gives false when `ccx` does not start.
fn solver_ready() -> bool {
    let exe = Ccx::new().exe;
    match Command::new(&exe).arg("-v").stdin(Stdio::null()).output() {
        Ok(_) => true,
        Err(e) => {
            eprintln!(
                "fh validate: cannot start ccx ({}): {e}. fh needs CalculiX: install it as \
                 docs/ccx.md tells, then put ccx on PATH or set FH_CCX",
                exe.display()
            );
            false
        }
    }
}

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut root = PathBuf::from(".");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = it.next().ok_or("--root needs a directory")?.into(),
            s => return Err(format!("unknown option: {s}")),
        }
    }
    let cases = match load_all(&root.join("validation/cases")) {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => return Err(format!("no cases under {}", root.display())),
        Err(e) => {
            eprintln!("fh validate: {e}");
            return Ok(ExitCode::from(1));
        }
    };
    if !fixtures_ok(&cases) {
        return Ok(ExitCode::from(1));
    }
    if !solver_ready() {
        return Ok(ExitCode::from(1));
    }
    let reports: Vec<(&Case, Report)> = cases
        .iter()
        .map(|case| (case, run_case_with(case, &Subprocess::for_case(case))))
        .collect();
    Ok(print_reports(&reports))
}

/// Print each problem of the fixtures of the cases. True when there is none.
fn fixtures_ok(cases: &[Case]) -> bool {
    let mut ok = true;
    for case in cases {
        for problem in spec_problems(case) {
            eprintln!(
                "fh validate: {}: {problem}",
                case.dir.join("case.toml").display()
            );
            ok = false;
        }
    }
    ok
}

/// Print one line for each case, the quantity tables, and the summary. Exit code 1 on a failure.
fn print_reports(reports: &[(&Case, Report)]) -> ExitCode {
    let (mut pass, mut fail) = (0, 0);
    let mut lines = String::new();
    let mut tables = String::new();
    for (case, report) in reports {
        let status = match &report.outcome {
            Outcome::Pass => {
                pass += 1;
                "pass".to_owned()
            }
            Outcome::Fail(why) => {
                fail += 1;
                if why.contains("cannot run") {
                    format!("cannot run: {why}")
                } else {
                    format!("fail: {why}")
                }
            }
        };
        lines.push_str(&format!("{}\t{status}\n", case.id));
        for r in &report.rows {
            tables.push_str(&format!(
                "  {}\t{}\tcomputed={:.10e}\treference={:.10e}\ttolerance={:.3e}\tmargin={:.3e}\n",
                case.id, r.name, r.computed, r.reference, r.tolerance, r.margin
            ));
        }
    }
    print!("{lines}{tables}");
    println!("{} cases: {pass} pass, {fail} fail", reports.len());
    if fail == 0 && pass > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The problems between the quantities of a case and its mesh file: a mesh that does not parse,
/// and a set or step that the mesh lacks.
fn spec_problems(case: &Case) -> Vec<String> {
    let name = case.mesh.display();
    let model = match std::fs::read_to_string(&case.mesh)
        .map_err(|e| e.to_string())
        .and_then(|t| {
            fh_inp::parse_file(&name.to_string(), &t).map_err(|e| fh_inp::join_errors(&e))
        }) {
        Ok(m) => m,
        Err(e) => return vec![format!("mesh {name}: {e}")],
    };
    let mut out = Vec::new();
    for r in &case.references {
        let q = &r.quantity;
        let (kind, found) = match q.field {
            Field::S => ("element", model.elset(&q.set).is_some()),
            _ => ("node", model.nset(&q.set).is_some()),
        };
        if !found {
            out.push(format!(
                "mesh {name} has no {kind} set `{}` for `{}`",
                q.set, r.name
            ));
        }
        if !model.steps.iter().any(|s| {
            s.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(&q.step))
        }) {
            out.push(format!(
                "mesh {name} has no step `{}` for `{}`",
                q.step, r.name
            ));
        }
    }
    out
}
