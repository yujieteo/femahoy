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
//! The daemon starts no solver. `fh run` starts `ccx` through `fh_solver_ccx` (DESIGN.md
//! section 6), and a run does not depend on the daemon.
//!
//! One daemon at a time uses a state folder. A lock file makes this sure.

mod browser;
mod chat;
mod http;
mod json;
mod message;
mod serve;
mod store;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use message::Error;
pub use serve::{default_state_dir, serve, serve_until};
pub use store::{new_preview, new_run, Run};

/// `n` random bytes as hex digits, from `/dev/urandom`.
pub(crate) fn random_hex(n: usize) -> Result<String, Error> {
    let mut bytes = vec![0u8; n];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| Error::io("/dev/urandom", e))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub viewer: Option<PathBuf>,
}

pub struct Daemon {
    chat: Arc<chat::Chat>,
    server: http::Server,
    token: String,
    /// The lock of the state folder, free when the daemon ends.
    _lock: File,
}

impl Daemon {
    /// Lock the state folder, make a random token and listen on 127.0.0.1 at any free port.
    /// `options.viewer` is the folder of the wasm viewer (see `fh serve`).
    pub fn start_with_options(state_dir: &Path, options: Options) -> Result<Daemon, Error> {
        let (store, lock) = store::Store::open(state_dir)?;
        let store = Arc::new(store);
        let token = random_hex(32)?;
        let chat = chat::Chat::new(store.clone(), state_dir.to_owned(), token.clone());
        let server = http::Server::start(store, chat.clone(), token.clone(), options.viewer)
            .map_err(|e| Error::io("listen on 127.0.0.1", e))?;
        Ok(Daemon {
            chat,
            server,
            token,
            _lock: lock,
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
