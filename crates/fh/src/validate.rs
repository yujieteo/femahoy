//! `fh validate [--root DIR]`: load every case and run the harness.
//!
//! Each case runs through `ccx` (`$FH_CCX`, else `ccx` on PATH), or, when its checks are tables,
//! through OpenFOAM (`$FH_OPENFOAM`, else `openfoam` on PATH), GetDP (`$FH_GETDP`, else `getdp`
//! on PATH), OpenFOAM and CalculiX coupled by preCICE (`$FH_CCX_PRECICE`, else `ccx_preCICE` on
//! PATH), Chrono (`$FH_PYCHRONO`, else `python3` on PATH), LAMMPS (`$FH_LAMMPS`, else `lmp` on
//! PATH), Meep (`$FH_MEEP`, else `python3` on PATH) or the topology and shape optimization loops
//! around `ccx`. A table shows every quantity with its computed value, reference, tolerance and
//! margin. A bad fixture, a case that fails or a case that cannot run gives exit code 1. A case
//! whose solver does not start cannot run, and its line tells how to install that solver.

use fh::validation::{load_all, run_case_with, Case, Outcome, Report, Subprocess};
use std::path::PathBuf;
use std::process::ExitCode;

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
    let solve = Subprocess::default();
    let reports: Vec<(&Case, Report)> = cases
        .iter()
        .map(|case| (case, run_case_with(case, &solve)))
        .collect();
    Ok(print_reports(&reports))
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
