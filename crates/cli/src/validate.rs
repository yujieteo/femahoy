//! `fh validate [--root DIR]`: load every case and run the harness.
//!
//! `fh validate --convergence PATH` runs the convergence check of one case folder, or of every
//! case folder inside PATH. A class N case has one mesh, so it is skipped in a folder of cases
//! and it is an error when named directly.
//!
//! M1 has no solver, so each case reports `no solver` and the exit code is 0.
//! A bad fixture, or a case that fails, gives exit code 1.

use fh_validation::{load_all, run_case, run_convergence, Case, Class, Field, Outcome};
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
    let mut bad = false;
    for case in &cases {
        for problem in spec_problems(case) {
            eprintln!(
                "fh validate: {}: {problem}",
                case.dir.join("case.toml").display()
            );
            bad = true;
        }
    }
    if bad {
        return Ok(ExitCode::from(1));
    }
    let (mut pass, mut fail, mut no_solver) = (0, 0, 0);
    let mut lines = String::new();
    for case in &cases {
        let status = match run_case(case) {
            Outcome::Pass => {
                pass += 1;
                "pass".to_owned()
            }
            Outcome::Fail(why) => {
                fail += 1;
                format!("fail: {why}")
            }
            Outcome::NoSolver => {
                no_solver += 1;
                "no solver".to_owned()
            }
        };
        lines.push_str(&format!("{}\t{}\t{status}\n", case.id, case.class.letter()));
    }
    if pass + fail == 0 {
        println!("no solver");
    }
    print!("{lines}");
    println!(
        "{} cases: {pass} pass, {fail} fail, {no_solver} no solver",
        cases.len()
    );
    Ok(if fail == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
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
    let (mut pass, mut fail, mut no_solver) = (0, 0, 0);
    let mut lines = String::new();
    for case in &cases {
        let status = if matches!(case.class, Class::N { .. }) && !single {
            "skipped, one mesh".to_owned()
        } else {
            let result = match run_convergence(case) {
                Outcome::Pass => {
                    pass += 1;
                    "pass".to_owned()
                }
                Outcome::Fail(why) => {
                    fail += 1;
                    eprintln!("fh validate: {why}");
                    format!("fail: {why}")
                }
                Outcome::NoSolver => {
                    no_solver += 1;
                    "no solver".to_owned()
                }
            };
            format!("{} meshes\t{result}", case.meshes.len())
        };
        lines.push_str(&format!("{}\t{}\t{status}\n", case.id, case.class.letter()));
    }
    if pass + fail == 0 && no_solver > 0 {
        println!("no solver");
    }
    print!("{lines}");
    println!(
        "{} cases: {pass} pass, {fail} fail, {no_solver} no solver",
        cases.len()
    );
    Ok(if fail == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// The problems between the quantities of a case and its mesh files: a mesh that does not
/// parse, an element type that the case does not list, and a set or step that the mesh lacks.
fn spec_problems(case: &Case) -> Vec<String> {
    let mut out = Vec::new();
    for mesh in &case.meshes {
        let name = mesh.display();
        let model = match std::fs::read_to_string(mesh)
            .map_err(|e| e.to_string())
            .and_then(|t| fh_inp::parse(&t).map_err(|e| e.to_string()))
        {
            Ok(m) => m,
            Err(e) => {
                out.push(format!("mesh {name}: {e}"));
                continue;
            }
        };
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
    out
}
