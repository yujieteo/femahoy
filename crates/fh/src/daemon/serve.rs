//! `fh serve`: run the daemon until a stop signal. The daemon listens on 127.0.0.1 only, and
//! no option changes the address. It makes a random token at each start and prints it in the
//! address of the page. Each request must carry the token, and a request from a page of
//! another `Origin` fails (`http` module).
//! The page is the wasm viewer, from `$FH_VIEWER` or the `viewer` folder beside `fh`.
//! At start, `fh serve` checks that the agent starts and that Claude Code has a login. When it
//! does not, the chat of the page tells the person what to do. On macOS, `fh serve` then opens the
//! page in the browser (`browser` module). On other systems it only prints the address.
//! The state folder `DIR` is `$FH_STATE` (see `default_state_dir`). Ctrl+C stops the daemon.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::browser::{no_open, open_page};
use super::chat::{agent_problem, agent_program};
use super::{Daemon, Options};

const USAGE: &str = "usage: fh serve";

/// Run `fh serve` with the arguments after `serve`, until SIGINT, SIGTERM or SIGHUP.
pub fn serve(args: &[String]) -> Result<ExitCode, String> {
    if let Some(arg) = args.first() {
        return Err(format!("unknown option: {arg}\n{USAGE}"));
    }
    let state_dir =
        default_state_dir().ok_or_else(|| format!("no home folder, so set FH_STATE\n{USAGE}"))?;
    stop::install();
    Ok(serve_until(&state_dir, &stop::STOP, !no_open()))
}

/// Run the daemon on `state_dir` until `stop` is true. With `open`, it opens the page in the
/// browser. A daemon that cannot start prints the cause and gives exit code 1.
fn serve_until(state_dir: &Path, stop: &AtomicBool, open: bool) -> ExitCode {
    let options = Options {
        viewer: viewer_dir(),
    };
    let viewer = options.viewer.clone();
    let daemon = match Daemon::start_with_options(state_dir, options) {
        Ok(daemon) => daemon,
        Err(e) => {
            let dir = state_dir.display();
            eprintln!("fh serve: {dir}: {e}");
            return ExitCode::from(1);
        }
    };
    let url = format!(
        "http://127.0.0.1:{}/?token={}",
        daemon.port(),
        daemon.token()
    );
    println!("fh serve: {url}");
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
    if let Some(problem) = agent_problem(&agent_program()) {
        println!("fh serve: {problem}");
        daemon.chat.setup(&problem);
    }
    if open {
        match open_page(&url) {
            Ok(()) => println!("fh serve: opened the page in the browser"),
            Err(e) => println!("fh serve: no browser ({e}). Open the address above."),
        }
    }
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    // The drop stops the server and the agent.
    drop(daemon);
    println!("fh serve: stopped");
    ExitCode::SUCCESS
}

/// The folder of the wasm viewer files: `$FH_VIEWER`, else `viewer` beside the `fh`
/// program. `None` when the folder has no `fh_viewer_bg.wasm`.
pub(super) fn viewer_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("FH_VIEWER")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(std::env::current_exe().ok()?.parent()?.join("viewer")))?;
    dir.join("fh_viewer_bg.wasm").is_file().then_some(dir)
}

/// `$FH_STATE`, else `$XDG_STATE_HOME/femahoy`, else `~/.local/state/femahoy`. The daemon
/// gives `FH_STATE` to the agent of the chat.
pub fn default_state_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var("FH_STATE") {
        return Some(PathBuf::from(dir));
    }
    let base = var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(var("HOME")?).join(".local/state")))?;
    Some(base.join("femahoy"))
}

/// SIGINT, SIGTERM and SIGHUP stop the daemon in order.
mod stop {
    use std::sync::atomic::AtomicBool;

    pub static STOP: AtomicBool = AtomicBool::new(false);

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
}
