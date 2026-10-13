//! The `fh` command line binary.

mod check;
mod info;
mod measure;
mod mesh;
mod rules;
mod validate;
mod view;

use std::process::ExitCode;

/// Subcommands planned in DESIGN.md section 13 and not implemented yet.
const PLANNED: &[&str] = &["results", "plot", "serve", "init", "team", "skills"];

fn usage() -> String {
    let mut text = String::from(
        "usage: fh [-h | --help] <command>\n\ncommands:\n  \
         check     parse .inp files (fh check [--root DIR] [FILE.inp...])\n  \
         mesh      run gmsh on a .geo or .step file (fh mesh FILE [-o OUT.msh] [--order 1|2])\n  \
         validate  run the validation suite (fh validate [--root DIR] [--convergence PATH])\n  \
         info      check units and mesh quality (fh info FILE.step|FILE.msh [--case DIR])\n  \
         view      show the mesh and its outline in 3D (fh view MODEL.inp)\n\n\
         TOON numbers are rounded to 4 significant figures (header key `precision`).\n\
         Exact values are in the VTU files.\n\n\
         planned subcommands:\n",
    );
    for name in PLANNED {
        text.push_str(&format!("  {name:<10}not implemented\n"));
    }
    text
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
        Some("info") => info::run(&args[1..]),
        Some("view") => view::run(&args[1..]),
        _ => {
            if args.len() == 1 && PLANNED.contains(&args[0].as_str()) {
                eprintln!("fh {}: not implemented", args[0]);
            } else {
                eprintln!("fh: unknown argument: {}", args.join(" "));
            }
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    outcome.unwrap_or_else(|message| {
        eprintln!("fh {}: {message}", args[0]);
        ExitCode::from(2)
    })
}
