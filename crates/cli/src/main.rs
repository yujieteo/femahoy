//! The `fh` command line binary.

mod check;
mod info;
mod measure;
mod mesh;
mod model;
mod rules;
mod run;
mod validate;

use std::process::ExitCode;

fn usage() -> &'static str {
    "usage: fh [-h | --help] <command>\n\ncommands:\n  \
         check     parse .inp files (fh check [--root DIR] [FILE.inp...])\n  \
         mesh      run gmsh on a .geo or .step file (fh mesh FILE [-o OUT.msh] [--order 1|2])\n  \
         model     write a solver input from a .msh file and the case (fh model FILE.msh --case DIR [-o OUT.inp])\n  \
         validate  run the validation suite (fh validate [--root DIR] [--convergence PATH])\n  \
         info      check units and mesh quality (fh info FILE.step|FILE.msh [--case DIR])\n  \
         run       solve a model or a case on the daemon and open the results in the browser (fh run MODEL.inp|CASE_DIR|--example NAME [--preview])\n  \
         serve     run the daemon, the browser viewer and the chat on 127.0.0.1 (fh serve [--state DIR] [--port N] [--no-open] | --stop)\n\n\
                  TOON numbers are rounded to 4 significant figures (header key `precision`).\n\
         Exact values are in the VTU files.\n"
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    let outcome = match first {
        None | Some("-h" | "--help") if args.len() <= 1 => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Some("check") => check::run(&args[1..]),
        Some("mesh") => mesh::run(&args[1..]),
        Some("validate") => validate::run(&args[1..]),
        Some("model") => model::run(&args[1..]),
        Some("info") => info::run(&args[1..]),
        Some("run") => run::run(&args[1..]),
        Some("serve") => fh_daemon::serve(&args[1..], measure::Subprocess::locate().exe),
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
