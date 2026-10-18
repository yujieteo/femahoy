//! `fh serve [--state DIR] [--port N] [--no-open]`: run the daemon until a stop signal. The daemon listens on
//! 127.0.0.1 only, and no option changes the address. It makes a random token at each start
//! and writes the port and the token to `DIR/daemon.info` (mode 0600). Each request must
//! carry the token, and a request from a page of another `Origin` fails (`http` module).
//! The page is the wasm viewer, from `$FH_VIEWER` or the `viewer` folder beside `fh`, with
//! the terminal sidebar. The panes are plain PTYs of the daemon.
//! At start, `fh serve` opens the page in the browser (`browser` module).
//!
//! `fh serve --stop [--state DIR]` stops the daemon of the state folder, for example the
//! daemon that `fh run` started in the background.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fh_solver_api::SubprocessSolver;

use crate::browser::{no_open, open_page};
use crate::service::LOCK_FILE;
use crate::{Client, Config, Daemon, Options};

const USAGE: &str = "usage: fh serve [--state DIR] [--port N] [--no-open]\n       \
                     fh serve --stop [--state DIR]";

/// The longest time that `fh serve --stop` waits for the daemon to end its runs.
const STOP_WAIT: Duration = Duration::from_secs(30);

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
    let mut options = Options::default();
    let (mut no_open_option, mut stop_option, mut start_option) = (false, false, false);
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
            "--no-open" => no_open_option = true,
            "--stop" => stop_option = true,
            _ => return Err(format!("unknown option: {arg}\n{USAGE}")),
        }
        // The options other than --state and --stop are for a start.
        start_option |= !matches!(arg.as_str(), "--state" | "--stop");
    }
    let state_dir = state
        .or_else(default_state_dir)
        .ok_or_else(|| format!("no home folder, so give the state folder\n{USAGE}"))?;
    if stop_option {
        if start_option {
            return Err(format!("--stop takes only --state DIR\n{USAGE}"));
        }
        return Ok(stop_daemon(&state_dir));
    }
    let config = Config {
        state_dir: state_dir.clone(),
        solver: SubprocessSolver::new(solver),
    };
    options.viewer = viewer_dir();
    let viewer = options.viewer.clone();
    let daemon = match Daemon::start_with_options(config, port, options) {
        Ok(daemon) => daemon,
        Err(e) => {
            let dir = state_dir.display();
            eprintln!("fh serve: {dir}: {} ({})", e.message, e.code);
            if let Some(c) = Client::discover(&state_dir)
                .ok()
                .filter(|c| c.status().is_ok())
            {
                eprintln!(
                    "fh serve: its page is {}. Stop it with: fh serve --stop --state {dir}",
                    c.page_url("")
                );
            }
            return Ok(ExitCode::from(1));
        }
    };
    let url = daemon.client().page_url("");
    println!("fh serve: {url}");
    match &viewer {
        Some(dir) => println!("fh serve: viewer {}", dir.display()),
        None => println!(
            "fh serve: the wasm viewer is not built (scripts/build-viewer.sh), so the page \
             shows only an error"
        ),
    }
    let panes = daemon.panes().status();
    println!("fh serve: panes {} ({})", panes.kind, panes.detail);
    println!(
        "fh serve: state folder {}. Stop with Ctrl+C.",
        state_dir.display()
    );
    if !no_open(no_open_option) {
        match open_page(&state_dir, &url) {
            Ok(()) => println!("fh serve: opened the page in the browser"),
            Err(e) => println!("fh serve: no browser ({e}). Open the address above."),
        }
    }
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

/// `fh serve --stop`: send a stop signal to the daemon that answers for `state_dir`, then
/// wait until it frees the state folder. The daemon ends its active runs as `interrupted`
/// (the run contract). No daemon to stop is not an error.
fn stop_daemon(state_dir: &Path) -> ExitCode {
    let dir = state_dir.display();
    // Only the daemon with the token of daemon.info answers, so the pid is its own.
    let status = Client::discover(state_dir).and_then(|c| c.status());
    let pid = match status {
        Ok(m) => m.get("pid").and_then(|p| p.parse::<u32>().ok()),
        Err(e) => {
            println!("fh serve: no daemon answers for {dir} ({})", e.code);
            return ExitCode::SUCCESS;
        }
    };
    let Some(pid) = pid else {
        eprintln!("fh serve: the daemon of {dir} gives no pid");
        return ExitCode::from(1);
    };
    if let Err(e) = stop::terminate(pid) {
        eprintln!("fh serve: cannot stop the daemon (pid {pid}): {e}");
        return ExitCode::from(1);
    }
    let deadline = Instant::now() + STOP_WAIT;
    while Instant::now() < deadline {
        let lock = OpenOptions::new()
            .write(true)
            .open(state_dir.join(LOCK_FILE));
        // The daemon frees the lock when its runs have ended.
        if lock.is_ok_and(|f| f.try_lock().is_ok()) {
            println!("fh serve: stopped the daemon (pid {pid}) of {dir}");
            return ExitCode::SUCCESS;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    eprintln!(
        "fh serve: the daemon (pid {pid}) did not stop in {} s",
        STOP_WAIT.as_secs()
    );
    ExitCode::from(1)
}

/// `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. On Windows
/// `%LOCALAPPDATA%\femahoy`.
pub fn default_state_dir() -> Option<PathBuf> {
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

    /// Send SIGTERM to the process `pid`.
    #[cfg(unix)]
    pub fn terminate(pid: u32) -> Result<(), String> {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let pid = i32::try_from(pid).map_err(|_| format!("bad pid {pid}"))?;
        // SAFETY: kill only sends a signal. The pid comes from the daemon that answered.
        match unsafe { kill(pid, 15) } {
            0 => Ok(()),
            _ => Err(std::io::Error::last_os_error().to_string()),
        }
    }

    /// End the process `pid`. Windows has no stop signal, so its runs end as after a daemon
    /// failure (`interrupted` at the next start).
    #[cfg(not(unix))]
    pub fn terminate(pid: u32) -> Result<(), String> {
        let pid = pid.to_string();
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid, "/F"])
            .status()
            .map_err(|e| e.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("taskkill: {status}"))
    }
}
