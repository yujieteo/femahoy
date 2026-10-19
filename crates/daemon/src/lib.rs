//! The daemon of femahoy (`fh serve`, DESIGN.md section 4.1). This crate has the run
//! contract (roadmap item D1): one contract for runs that the CLI and the UI share.
//!
//! - **Runs.** A submit names the absolute path of an input file. The daemon copies the
//!   file and its `*INCLUDE` files into the folder of a new run, and the run solves the
//!   copy, so a later edit of a file does not change the run. A submit can set the time and
//!   memory limit of the run. Each run has an ID, one record on disk and one state
//!   ([`RunState`]).
//! - **Previews.** A preview copies the input file with no run, so that the page shows the
//!   model with its supports and loads before the solve.
//! - **Bounded queues.** At most [`MAX_QUEUED`] runs wait. Each run keeps its last
//!   [`EVENT_QUEUE`] events in memory and an event log of at most [`EVENT_LOG`] bytes.
//!   Clients read events by sequence number, so a slow client never grows a queue.
//! - **Access.** The token and `Origin` rules are in the `http` module documentation.
//! - **Chat.** The daemon runs the agent, a Claude Code process in headless mode, and keeps
//!   its actions as chat entries for the page (`chat`, `docs/chat.md`).
//!
//! A run starts `ccx` through `fh_solver_ccx` in the daemon process (DESIGN.md section 9).
//! The contract only starts it, cancels it and keeps the record.
//!
//! # Which process stays alive
//!
//! The daemon owns every run. A browser and the CLI are only clients.
//!
//! 1. A client that closes or disconnects stops nothing. The run continues, and the client
//!    reads the same record when it connects again.
//! 2. Only a cancel command stops a run on purpose.
//! 3. The solver never lives longer than the daemon. A daemon that stops cancels its runs.
//!    When a daemon fails, a guard process kills the process group of `ccx`.
//! 4. A later daemon shows a run that an earlier daemon did not end as `interrupted`. It
//!    never starts such a run again.
//! 5. One daemon at a time uses a state folder. A lock file makes this sure.

mod browser;
mod chat;
mod client;
mod contract;
mod http;
mod json;
mod serve;
mod service;

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use browser::{no_open, open_page};
pub use client::Client;
pub use contract::{Error, Event, Events, Message, RunRecord, RunState};
pub use serve::{default_state_dir, serve, serve_until};
use service::Service;
pub use service::{Config, Limits, EVENT_LOG, EVENT_QUEUE, EVENT_TEXT, MAX_QUEUED};

/// The file in the state folder that tells clients the port and the token.
pub const INFO_FILE: &str = "daemon.info";

/// `n` random bytes as hex digits, from `/dev/urandom`.
pub(crate) fn random_hex(n: usize) -> Result<String, Error> {
    let mut bytes = vec![0u8; n];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| Error::io("/dev/urandom", e))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Write a file that only the user can read.
fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let err = |e| Error::io(&path.display().to_string(), e);
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(err)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub viewer: Option<PathBuf>,
}

pub struct Daemon {
    service: Arc<Service>,
    chat: Arc<chat::Chat>,
    server: http::Server,
    token: String,
    info: PathBuf,
}

impl Daemon {
    /// Open the service, make a random token, listen on 127.0.0.1 at `port` (0 for any free
    /// port) and write `daemon.info` for the clients. `options.viewer` is the folder of the
    /// wasm viewer (see `fh serve`).
    pub fn start_with_options(
        config: Config,
        port: u16,
        options: Options,
    ) -> Result<Daemon, Error> {
        let info = config.state_dir.join(INFO_FILE);
        let state_dir = config.state_dir.clone();
        let service = Arc::new(Service::open(config)?);
        let token = random_hex(32)?;
        let chat = chat::Chat::new(service.clone(), state_dir, token.clone());
        let server = http::Server::start(
            service.clone(),
            chat.clone(),
            port,
            token.clone(),
            options.viewer,
        )
        .map_err(|e| Error::io("listen on 127.0.0.1", e))?;
        let mut m = Message::new();
        m.set("port", server.port());
        m.set("token", &token);
        m.set("pid", std::process::id());
        write_private(&info, &m.to_text())?;
        Ok(Daemon {
            service,
            chat,
            server,
            token,
            info,
        })
    }

    pub fn port(&self) -> u16 {
        self.server.port()
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// A client with the token of this daemon.
    pub fn client(&self) -> Client {
        Client::new(self.port(), &self.token)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info);
        self.server.stop();
        self.chat.stop();
        self.service.stop();
    }
}
