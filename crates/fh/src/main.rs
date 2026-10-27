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
         report    write 1 HTML file with a text, the viewer, the results of runs and plots of curves (fh report TEXT.html -o REPORT.html [--run RUN]... [--curve CURVE.csv]... [--viewer FILE.js] [--one-step])\n  \
         fit       fit a model to a curve by least squares (fh fit MODEL CURVE.csv [--series NAME] [-o OUT.csv])\n  \
         guide     print the how-to guide of an analysis for the agent (fh guide NAME)\n  \
         serve     run the daemon, the browser viewer and the chat on 127.0.0.1 (fh serve)\n"
}

/// Set each variable of the file `solvers.env` beside the `fh` program that the environment does
/// not set. `scripts/install.sh` writes the file: 1 line `FH_NAME=PATH` for each solver that it
/// installed. A child process, for example the agent of `fh serve`, gets the variables.
fn load_solvers() {
    let Some(file) = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("solvers.env")))
    else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(file) else {
        return;
    };
    for (name, value) in text.lines().filter_map(|line| line.split_once('=')) {
        if std::env::var_os(name).is_none_or(|v| v.is_empty()) {
            std::env::set_var(name, value);
        }
    }
}

fn main() -> ExitCode {
    load_solvers();
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
        Some("guide") => fh::guide::command(&args[1..]),
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
