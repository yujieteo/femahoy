//! `fh validate [--root DIR]`: load every case and run the harness.
//!
//! M1 has no solver, so each case reports `no solver` and the exit code is 0.
//! A bad fixture, or a case that fails, gives exit code 1.

use fh_validation::{load_all, run_case, Outcome};
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
