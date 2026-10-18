//! The core solver as a protocol v2 executable:
//! `fh-solve <input.inp> <run-dir> [--restart] [--formats text,vtu,vtk]
//! [--restart-every N] [--restart-minutes T]`.
//!
//! Event lines go to stdout. A `cancel` line on stdin stops the run. The end of
//! stdin also stops it when `FH_CANCEL_ON_EOF` is `1`, which the runner sets.
//! `FH_THREADS` sets the number of element threads.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fh_core_solver::{input_hash, input_units, run_model, Formats, Input, Options};
use fh_solver_api::{watch_stdin, PROTOCOL_HEADER};

/// Exit code of a run that the user cancelled.
const CANCELLED: u8 = 130;

fn run(
    inp: &Path,
    run_dir: &Path,
    restart: bool,
    mut options: Options,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(inp).map_err(|e| format!("{}: {e}", inp.display()))?;
    let model = fh_inp::parse_file(&inp.display().to_string(), &text)
        .map_err(|e| fh_inp::join_errors(&e))?;
    if let Some(n) = std::env::var("FH_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        options.threads = n;
    }
    let out = std::io::stdout();
    let mut emit = |line: &str| {
        let mut o = out.lock();
        let _ = writeln!(o, "{line}");
        let _ = o.flush();
    };
    let name = inp.file_name().map_or_else(
        || inp.display().to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    let input = Input {
        name: &name,
        hash: input_hash(&text),
        units: input_units(&text),
    };
    run_model(
        &model,
        &input,
        run_dir,
        &options,
        restart,
        &mut emit,
        &|| cancel.load(Ordering::Relaxed),
    )
    .map(|_| ())
    .map_err(|e| {
        if e.is_cancelled() {
            "cancelled".into()
        } else {
            e.to_string()
        }
    })
}

fn main() -> ExitCode {
    let usage = || {
        eprintln!(
            "usage: fh-solve <input.inp> <run-dir> [--restart] [--formats text,vtu,vtk] \
[--restart-every N] [--restart-minutes T]"
        );
        ExitCode::from(2)
    };
    let mut args = Vec::new();
    let mut restart = false;
    let mut options = Options::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--restart" => restart = true,
            "--formats" => match it.next().map(|l| Formats::parse(&l)) {
                Some(Ok(f)) => options.formats = f,
                Some(Err(e)) => {
                    eprintln!("{e}");
                    return ExitCode::from(2);
                }
                None => return usage(),
            },
            "--restart-every" => match it.next().and_then(|v| v.parse::<usize>().ok()) {
                Some(n) if n > 0 => options.restart_every = n,
                _ => return usage(),
            },
            "--restart-minutes" => match it.next().and_then(|v| v.parse::<f64>().ok()) {
                Some(t) if t > 0.0 => options.restart_minutes = Some(t),
                _ => return usage(),
            },
            _ => args.push(a),
        }
    }
    if args.len() != 2 {
        return usage();
    }
    println!("{PROTOCOL_HEADER}");
    let _ = std::io::stdout().flush();
    let cancel = watch_stdin();
    match run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        restart,
        options,
        cancel,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e == "cancelled" => {
            eprintln!("cancelled");
            ExitCode::from(CANCELLED)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
