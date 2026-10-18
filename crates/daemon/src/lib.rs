//! The daemon of femahoy (`fh serve`, DESIGN.md section 4.1). This crate has the run
//! contract (roadmap item D1): one contract for runs that the CLI and the UI share.
//!
//! - **Projects.** A project is a folder. The daemon gives each root folder one project ID.
//! - **Fixed input revisions.** A submit copies the input file into a read-only revision.
//!   The run solves the revision, so a later edit of the file does not change the run.
//!   The same bytes give the same revision ID, from the FNV-1a hash that the `fh.run`
//!   summary also gives as `source_hash`.
//! - **Runs.** Each run has an ID, one record on disk and one state ([`RunState`]).
//! - **Commands.** A submit and a cancel each carry a request ID from the client. The daemon
//!   keeps the acknowledgement ([`Ack`]), so a retry after a lost answer changes nothing.
//! - **Bounded queues.** At most [`MAX_QUEUED`] runs wait. Each run keeps its last
//!   [`EVENT_QUEUE`] events in memory and an event log of at most [`EVENT_LOG`] bytes.
//!   Clients read events by sequence number, so a slow client never grows a queue.
//! - **Access.** The token and `Origin` rules are in the `http` module documentation.
//!
//! The solve path stays the existing subprocess protocol v2 (`fh-solve`, DESIGN.md
//! section 9.3). The contract only starts it, cancels it and keeps the record.
//!
//! # Which process stays alive
//!
//! The daemon owns every run. A browser, the native UI and the CLI are only clients.
//!
//! 1. A client that closes or disconnects stops nothing. The run goes on, and the client
//!    reads the same record when it connects again.
//! 2. Only a cancel command stops a run on purpose.
//! 3. The solver never lives longer than the daemon. A daemon that stops cancels its runs.
//!    A daemon that fails closes the stdin of the solver, and protocol v2 then stops it.
//! 4. At the next start, the daemon marks each run that was still active as `interrupted`,
//!    or as `cancelled` when a cancel was pending. It never starts such a run again by
//!    itself. The run folder keeps the restart file of the solver.
//! 5. One daemon at a time uses a state folder. A lock file makes this sure.

mod client;
mod contract;
mod http;
mod serve;
mod service;

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use client::{new_request_id, Client};
pub use contract::{
    valid_project, valid_request, valid_run, Ack, Command, Error, Event, Events, Message, Outcome,
    RunRecord, RunState, CONTRACT,
};
pub use serve::{serve, serve_until};
pub use service::{
    Config, Service, EVENT_BATCH, EVENT_LOG, EVENT_QUEUE, EVENT_TEXT, MAX_ACKS, MAX_QUEUED,
    MAX_WAIT,
};

/// The file in the state folder that tells clients the port and the token.
pub const INFO_FILE: &str = "daemon.info";

/// `n` random bytes as hex digits, from the random source of the operating system.
pub(crate) fn random_hex(n: usize) -> String {
    let mut bytes = vec![0u8; n];
    let from_os = File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if from_os.is_err() {
        // No /dev/urandom, for example on Windows. The keys of `RandomState` come from the
        // random source of the operating system.
        use std::hash::{BuildHasher, Hasher};
        for (i, chunk) in bytes.chunks_mut(8).enumerate() {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_usize(i);
            chunk.copy_from_slice(&h.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write a file that only the user can read.
fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let err = |e| Error::io(&path.display().to_string(), e);
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(err)?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(err)?;
    std::fs::rename(&tmp, path).map_err(err)
}

/// A running daemon: the run service and its HTTP transport on 127.0.0.1.
pub struct Daemon {
    service: Arc<Service>,
    server: http::Server,
    token: String,
    info: PathBuf,
}

impl Daemon {
    /// Open the service, make a random token, listen on 127.0.0.1 at `port` (0 for any free
    /// port) and write `daemon.info` for the clients.
    pub fn start(config: Config, port: u16) -> Result<Daemon, Error> {
        Daemon::start_with_viewer(config, port, None)
    }

    /// `start`, and serve the wasm viewer from the folder `viewer` (see `fh serve`).
    pub fn start_with_viewer(
        config: Config,
        port: u16,
        viewer: Option<PathBuf>,
    ) -> Result<Daemon, Error> {
        let info = config.state_dir.join(INFO_FILE);
        let service = Arc::new(Service::open(config)?);
        let token = random_hex(32);
        let server = http::Server::start(service.clone(), port, token.clone(), viewer)
            .map_err(|e| Error::io("listen on 127.0.0.1", e))?;
        let mut m = Message::new();
        m.set("port", server.port());
        m.set("token", &token);
        m.set("pid", std::process::id());
        m.set("instance", service.instance());
        write_private(&info, &m.to_text())?;
        Ok(Daemon {
            service,
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

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// A client with the token of this daemon.
    pub fn client(&self) -> Client {
        Client::new(self.port(), &self.token)
    }

    /// Stop the daemon. Its runs end as `interrupted` (see the crate documentation).
    pub fn stop(self) {}
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.info);
        self.server.stop();
        self.service.stop();
    }
}
