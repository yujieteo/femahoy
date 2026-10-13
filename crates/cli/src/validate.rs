//! `fh validate [--root DIR]`: load every case and run the harness.
//!
//! `fh validate --convergence PATH` runs the convergence check of one case folder, or of every
//! case folder inside PATH. A class N case has one mesh, so it is skipped in a folder of cases
//! and it is an error when named directly.
//!
//! Each case runs through `fh-solve` (`$FH_SOLVE`, or the binary next to `fh`). A table shows
//! every quantity with its computed value, reference, tolerance and margin.
//! A bad fixture, a case that fails, or a case that cannot run gives exit code 1.

use crate::measure::Subprocess;
use fh_validation::{
    load_all, run_case_with, run_convergence_with, Case, Class, Field, Outcome, Report,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut root = PathBuf::from(".");
    let mut convergence: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--convergence" => {
                convergence = Some(it.next().ok_or("--convergence needs a path")?.into())
            }
            "--root" => root = it.next().ok_or("--root needs a directory")?.into(),
            s => return Err(format!("unknown option: {s}")),
        }
    }
    if let Some(path) = convergence {
        return convergence_run(&path);
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
    let solver = Subprocess::locate();
    let reports: Vec<(&Case, String, Report)> = cases
        .iter()
        .map(|c| (c, String::new(), run_case_with(c, &solver)))
        .collect();
    Ok(print_reports(&reports, reports.len()))
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
fn print_reports(reports: &[(&Case, String, Report)], total: usize) -> ExitCode {
    let (mut pass, mut fail) = (0, 0);
    let mut lines = String::new();
    let mut tables = String::new();
    for (case, prefix, report) in reports {
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
            Outcome::NoSolver => {
                fail += 1;
                "cannot run: no solver".to_owned()
            }
        };
        lines.push_str(&format!(
            "{}\t{}\t{prefix}{status}\n",
            case.id,
            case.class.letter()
        ));
        for r in &report.rows {
            tables.push_str(&format!(
                "  {}\t{}\tcomputed={:.10e}\treference={:.10e}\ttolerance={:.3e}\tmargin={:.3e}\t{}\n",
                case.id, r.name, r.computed, r.reference, r.tolerance, r.margin, r.note
            ));
        }
    }
    print!("{lines}{tables}");
    println!("{total} cases: {pass} pass, {fail} fail");
    if fail == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `--convergence PATH`: PATH is a case folder, or a folder of case folders.
fn convergence_run(path: &Path) -> Result<ExitCode, String> {
    let single = path.join("case.toml").is_file();
    let loaded = if single {
        Case::load(path).map(|c| vec![c])
    } else {
        load_all(path)
    };
    let cases = match loaded {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => return Err(format!("no cases under {}", path.display())),
        Err(e) => {
            eprintln!("fh validate: {e}");
            return Ok(ExitCode::from(1));
        }
    };
    // The same fixture checks as a validation run.
    if !fixtures_ok(&cases) {
        return Ok(ExitCode::from(1));
    }
    let solver = Subprocess::locate();
    let mut reports: Vec<(&Case, String, Report)> = Vec::new();
    let mut skipped = Vec::new();
    for case in &cases {
        if matches!(case.class, Class::N { .. }) && !single {
            skipped.push(case);
        } else {
            let report = run_convergence_with(case, &solver);
            if let Outcome::Fail(why) = &report.outcome {
                eprintln!("fh validate: {why}");
            }
            reports.push((case, format!("{} meshes\t", case.meshes.len()), report));
        }
    }
    for case in &skipped {
        println!("{}\t{}\tskipped, one mesh", case.id, case.class.letter());
    }
    Ok(print_reports(&reports, reports.len() + skipped.len()))
}

/// The problems between the quantities of a case and its mesh files: a mesh that does not
/// parse, an element type that the case does not list, and a set or step that the mesh lacks.
fn spec_problems(case: &Case) -> Vec<String> {
    let mut out = Vec::new();
    // The element types of the meshes. `None` when a mesh does not parse.
    let mut seen: Option<std::collections::HashSet<String>> = Some(Default::default());
    for mesh in &case.meshes {
        let name = mesh.display();
        let model = match std::fs::read_to_string(mesh)
            .map_err(|e| e.to_string())
            .and_then(|t| fh_inp::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(m) => m,
            Err(e) => {
                out.push(format!("mesh {name}: {e}"));
                seen = None;
                continue;
            }
        };
        if let Some(seen) = &mut seen {
            seen.extend(model.elements.iter().map(|e| e.element_type.clone()));
        }
        for el in &model.elements {
            if !case.element_types.contains(&el.element_type) {
                out.push(format!(
                    "mesh {name} has element type {} that `element_types` does not list",
                    el.element_type
                ));
                break;
            }
        }
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
    }
    // The case runs once for each listed type, thus each type needs a mesh.
    for kind in &case.element_types {
        if seen.as_ref().is_some_and(|s| !s.contains(kind)) {
            out.push(format!(
                "`element_types` lists {kind}, but no mesh has that element type"
            ));
        }
    }
    out
}
