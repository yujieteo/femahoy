//! The `fh` command line binary.

use std::process::ExitCode;

/// Subcommands planned in DESIGN.md section 13. None is implemented in M0.
const PLANNED: &[&str] = &[
    "check", "validate", "results", "plot", "serve", "init", "team", "skills",
];

fn usage() -> String {
    let mut text = String::from("usage: fh [-h | --help]\n\nplanned subcommands:\n");
    for name in PLANNED {
        text.push_str(&format!("  {name:<10}not implemented\n"));
    }
    text
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);
    if args.len() <= 1 && matches!(first, None | Some("-h" | "--help")) {
        print!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if args.len() == 1 && PLANNED.contains(&args[0].as_str()) {
        eprintln!("fh {}: not implemented", args[0]);
    } else {
        eprintln!("fh: unknown argument: {}", args.join(" "));
    }
    eprint!("{}", usage());
    ExitCode::from(2)
}
