//! The `fh` command line binary.

mod measure;
mod mesh;
mod model;
mod rules;
mod run;
mod validate;

use std::process::ExitCode;

fn usage() -> &'static str {
    "usage: fh [-h | --help] <command>\n\ncommands:\n  \
         mesh      run gmsh on a .geo or .step file (fh mesh FILE [-o OUT.msh])\n  \
         model     write a solver input from a .msh file and the case (fh model FILE.msh --case DIR [-o OUT.inp])\n  \
         validate  run the validation suite (fh validate [--root DIR])\n  \
         run       solve a model or a case on the daemon and open the results in the browser (fh run MODEL.inp|CASE_DIR [--preview])\n  \
         serve     run the daemon, the browser viewer and the chat on 127.0.0.1 (fh serve [--state DIR] [--port N] [--no-open] | --stop)\n"
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    let outcome = match first {
        None | Some("-h" | "--help") if args.len() <= 1 => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Some("mesh") => mesh::run(&args[1..]),
        Some("validate") => validate::run(&args[1..]),
        Some("model") => model::run(&args[1..]),
        Some("run") => run::run(&args[1..]),
        Some("serve") => fh_daemon::serve(&args[1..]),
        _ => {
            eprintln!("fh: unknown argument: {}", args.join(" "));
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    outcome.unwrap_or_else(|message| {
        eprintln!("fh {}: {message}", args[0]);
        ExitCode::from(2)
    })
}
