//! The `fh` command line binary.

mod mesh;
mod rules;
mod run;
mod validate;

use std::process::ExitCode;

fn usage() -> &'static str {
    "usage: fh [-h | --help] <command>\n\ncommands:\n  \
         mesh      mesh a .step file into CalculiX nodes, elements and sets (fh mesh FILE [options] [-o OUT.inp])\n  \
         validate  run the validation suite of DIR/validation/cases, DIR is . by default (fh validate [--root DIR])\n  \
         run       solve a CalculiX input, or an OpenFOAM, GetDP, preCICE, Chrono, LAMMPS, Meep, openEMS, topology or shape optimization case (fh run MODEL.inp|CASE [--timeout SECONDS])\n  \
         report    write 1 HTML file with a text, the viewer, the results of runs and plots of curves (fh report TEXT.html -o REPORT.html [--run RUN]... [--curve CURVE.csv]... [--view VIEW.toml] [--viewer FILE.js] [--one-step])\n  \
         fit       fit a model to a curve by least squares (fh fit MODEL CURVE.csv [--series NAME] [-o OUT.csv])\n  \
         serve     run the daemon, the browser viewer and the chat on 127.0.0.1 (fh serve)\n"
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
        Some("run") => run::run(&args[1..]),
        Some("report") => fh::daemon::report(&args[1..]),
        Some("fit") => fh::fit::command(&args[1..]),
        Some("serve") => fh::daemon::serve(&args[1..]),
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
