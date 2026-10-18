//! `fh serve [--state DIR] [--port N]`: run the daemon until a stop signal. The daemon listens
//! on 127.0.0.1 only, and no option changes the address. It makes a random token at each
//! start and writes the port and the token to `DIR/daemon.info` (mode 0600). Each request
//! must carry the token, and a request from a page of another `Origin` fails (`http` module).
//! The page is the wasm viewer, from `$FH_VIEWER` or the `viewer` folder beside `fh`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fh_solver_api::SubprocessSolver;

use crate::{Config, Daemon};

const USAGE: &str = "usage: fh serve [--state DIR] [--port N]";

/// Run `fh serve` with the arguments after `serve`, until SIGINT, SIGTERM or SIGHUP.
/// `solver` is the protocol v2 solver, for example `fh-solve`.
pub fn serve(args: &[String], solver: PathBuf) -> Result<ExitCode, String> {
    stop::install();
    serve_until(args, solver, &stop::STOP)
}

/// Run `fh serve` until `stop` is true. A usage error is an `Err`. A daemon that cannot
/// start prints the cause and gives exit code 1.
pub fn serve_until(
    args: &[String],
    solver: PathBuf,
    stop: &AtomicBool,
) -> Result<ExitCode, String> {
    let mut state = None;
    let mut port = 0;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || {
            rest.next()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--state" => state = Some(PathBuf::from(value()?)),
            "--port" => {
                port = value()?
                    .parse()
                    .map_err(|_| format!("--port needs a number from 0 to 65535\n{USAGE}"))?
            }
            _ => return Err(format!("unknown option: {arg}\n{USAGE}")),
        }
    }
    let state_dir = state
        .or_else(default_state_dir)
        .ok_or_else(|| format!("no home folder, so give the state folder\n{USAGE}"))?;
    let config = Config {
        state_dir: state_dir.clone(),
        solver: SubprocessSolver::new(solver),
    };
    let viewer = viewer_dir();
    let daemon = match Daemon::start_with_viewer(config, port, viewer.clone()) {
        Ok(daemon) => daemon,
        Err(e) => {
            let dir = state_dir.display();
            eprintln!("fh serve: {dir}: {} ({})", e.message, e.code);
            return Ok(ExitCode::from(1));
        }
    };
    println!(
        "fh serve: http://127.0.0.1:{}/?token={}",
        daemon.port(),
        daemon.token()
    );
    match &viewer {
        Some(dir) => println!("fh serve: viewer {}", dir.display()),
        None => println!(
            "fh serve: the wasm viewer is not built (scripts/build-viewer.sh), so the page \
             shows only an error"
        ),
    }
    println!(
        "fh serve: state folder {}. Stop with Ctrl+C.",
        state_dir.display()
    );
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    // The drop removes daemon.info and ends the active runs as `interrupted`.
    drop(daemon);
    println!("fh serve: stopped");
    Ok(ExitCode::SUCCESS)
}

/// The folder of the wasm viewer files: `$FH_VIEWER`, else `viewer` beside the `fh`
/// program. `None` when the folder has no `fh_viewer_bg.wasm`.
fn viewer_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("FH_VIEWER")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(std::env::current_exe().ok()?.parent()?.join("viewer")))?;
    dir.join("fh_viewer_bg.wasm").is_file().then_some(dir)
}

/// `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. On Windows
/// `%LOCALAPPDATA%\femahoy`.
fn default_state_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = if cfg!(windows) {
        PathBuf::from(var("LOCALAPPDATA")?)
    } else {
        var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(var("HOME")?).join(".local/state")))?
    };
    Some(base.join("femahoy"))
}

/// SIGINT, SIGTERM and SIGHUP stop the daemon in order. With no handler, as on Windows, the
/// signal ends the process. The solvers then see the end of their stdin and stop, and the
/// next start marks their runs `interrupted`.
mod stop {
    use std::sync::atomic::AtomicBool;

    pub static STOP: AtomicBool = AtomicBool::new(false);

    #[cfg(unix)]
    pub fn install() {
        extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn on_signal(_: i32) {
            STOP.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        // SIGHUP, SIGINT and SIGTERM have these numbers on Linux and macOS.
        for sig in [1, 2, 15] {
            // SAFETY: the handler only stores an atomic flag, which is async-signal-safe.
            unsafe {
                signal(sig, on_signal);
            }
        }
    }

    #[cfg(not(unix))]
    pub fn install() {}
}
