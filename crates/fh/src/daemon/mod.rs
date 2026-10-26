//! The daemon of femahoy (`fh serve`, DESIGN.md section 4.1): the page with the chat, the
//! list of the runs and the wasm viewer.
//!
//! - **Runs and previews.** `fh run` writes each run and each preview into its own folder of
//!   the state folder (`store` module). The daemon only reads these folders, so the page
//!   shows the model with its supports and loads before the solve, and the results of each run.
//! - **Access.** The token and `Origin` rules are in the `http` module documentation.
//! - **Chat.** The daemon runs the agent, a Claude Code process in headless mode, and keeps
//!   its actions as chat entries for the page (`chat`).
//!
//! The daemon starts no solver. `fh run` starts `ccx` through the `ccx` module (DESIGN.md
//! section 6), and a run does not depend on the daemon. The daemon writes only the file `token`
//! of the state folder, once, so any number of daemons can use one state folder.

mod browser;
mod chat;
mod http;
mod json;
mod message;
mod report;
mod serve;
mod store;

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use report::report;
pub use serve::{default_state_dir, serve};
pub use store::{new_preview, new_run};

/// `n` random bytes as hex digits, from `/dev/urandom`.
pub(crate) fn random_hex(n: usize) -> Result<String, String> {
    let mut bytes = vec![0u8; n];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("/dev/urandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The token of the daemon in `state_dir/token`. The first start writes a random token with
/// owner-only permissions, and each later start reads it, so the address of the page stays the
/// same. A token file that other users can read gives an error.
fn stored_token(state_dir: &Path) -> Result<String, String> {
    let path = state_dir.join("token");
    if !path.exists() {
        // Write a temporary file, then link it: two daemons that start at the same time
        // never read a part of the token.
        let token = random_hex(32)?;
        let temporary = state_dir.join(format!(".token-{}", random_hex(8)?));
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .and_then(|mut f| f.write_all(token.as_bytes()).map(|()| f))
            .and_then(|f| f.sync_all());
        let linked = written.and_then(|()| match fs::hard_link(&temporary, &path) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
            other => other,
        });
        let _ = fs::remove_file(&temporary);
        linked.map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let file = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mode = file
        .metadata()
        .map_err(|e| format!("{}: {e}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "{}: other users can read the token, so run chmod 600 on it",
            path.display()
        ));
    }
    let mut token = String::new();
    (&file)
        .read_to_string(&mut token)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let token = token.trim().to_owned();
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "{}: the token is not 64 hex digits, so remove the file to make a new token",
            path.display()
        ));
    }
    Ok(token)
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub viewer: Option<PathBuf>,
    /// The port on 127.0.0.1. 0 is any free port.
    pub port: u16,
}

pub struct Daemon {
    chat: Arc<chat::Chat>,
    server: http::Server,
    token: String,
}

impl Daemon {
    /// Read the stored token (`stored_token`) and listen on 127.0.0.1 at `options.port`.
    /// `options.viewer` is the folder of the wasm viewer (see `fh serve`).
    pub fn start_with_options(state_dir: &Path, options: Options) -> Result<Daemon, String> {
        let store = Arc::new(store::Store::open(state_dir)?);
        let token = stored_token(state_dir)?;
        let chat = chat::Chat::new(store.clone(), state_dir.to_owned(), token.clone());
        let port = options.port;
        let server = http::Server::start(store, chat.clone(), token.clone(), options.viewer, port)
            .map_err(|e| match e.kind() {
                ErrorKind::AddrInUse => format!(
                    "port {port} on 127.0.0.1 is in use. Stop the program that uses it, \
                     or set FH_PORT to a free port"
                ),
                _ => format!("listen on 127.0.0.1:{port}: {e}"),
            })?;
        Ok(Daemon {
            chat,
            server,
            token,
        })
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.server.stop();
        self.chat.stop();
    }
}
