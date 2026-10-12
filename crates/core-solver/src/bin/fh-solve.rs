//! The core solver as a protocol v2 executable:
//! `fh-solve <input.inp> <run-dir> [--restart] [--formats text,vtu,vtk]`.
//!
//! Event lines go to stdout. A `cancel` line on stdin, or the end of stdin,
//! stops the run. `FH_THREADS` sets the number of element threads.

use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fh_core_solver::{input_hash, run_model, Formats, Options};
use fh_solver_api::PROTOCOL_HEADER;

/// Exit code of a run that the user cancelled.
const CANCELLED: u8 = 130;

fn run(
    inp: &Path,
    run_dir: &Path,
    restart: bool,
    formats: Formats,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let text = std::fs::read_to_string(inp).map_err(|e| format!("{}: {e}", inp.display()))?;
    let model = fh_inp::parse(&text).map_err(|e| e.to_string())?;
    let mut options = Options {
        formats,
        ..Options::default()
    };
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
    run_model(
        &model,
        input_hash(&text),
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
        eprintln!("usage: fh-solve <input.inp> <run-dir> [--restart] [--formats text,vtu,vtk]");
        ExitCode::from(2)
    };
    let mut args = Vec::new();
    let (mut restart, mut formats) = (false, Formats::default());
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--restart" => restart = true,
            "--formats" => match it.next().map(|l| Formats::parse(&l)) {
                Some(Ok(f)) => formats = f,
                Some(Err(e)) => {
                    eprintln!("{e}");
                    return ExitCode::from(2);
                }
                None => return usage(),
            },
            _ => args.push(a),
        }
    }
    if args.len() != 2 {
        return usage();
    }
    println!("{PROTOCOL_HEADER}");
    let _ = std::io::stdout().flush();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        // Read in blocks and keep only the start of each line, so a stream with no
        // line ends (for example /dev/zero) cannot use memory without limit.
        let mut stdin = std::io::stdin().lock();
        let (mut block, mut line) = ([0u8; 4096], Vec::<u8>::new());
        'read: loop {
            let n = match stdin.read(&mut block) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &b in &block[..n] {
                if b == b'\n' {
                    if line.trim_ascii() == b"cancel" {
                        break 'read;
                    }
                    line.clear();
                } else if line.len() < 64 {
                    line.push(b);
                }
            }
        }
        flag.store(true, Ordering::Relaxed);
    });
    match run(
        Path::new(&args[0]),
        Path::new(&args[1]),
        restart,
        formats,
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
