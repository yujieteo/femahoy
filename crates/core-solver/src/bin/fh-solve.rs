//! The core solver as a protocol v1 executable: `fh-solve <input.inp> <run-dir>`.

use std::path::Path;
use std::process::ExitCode;

use fh_core_solver::CoreSolver;
use fh_solver_api::PROTOCOL_HEADER;

fn run(inp: &Path, run_dir: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(inp).map_err(|e| format!("{}: {e}", inp.display()))?;
    let model = fh_inp::parse(&text).map_err(|e| e.to_string())?;
    CoreSolver
        .solve(&model, run_dir)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: fh-solve <input.inp> <run-dir>");
        return ExitCode::from(2);
    }
    println!("{PROTOCOL_HEADER}");
    match run(Path::new(&args[0]), Path::new(&args[1])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
