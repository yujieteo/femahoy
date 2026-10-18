//! The pane backend (DESIGN.md section 4.3): the panes where teammates run. The backend
//! is herdr when herdr is installed (`herdr.rs`), else plain PTYs that the daemon owns.
//! Everything that shows or drives panes reads them from [`PaneBackend`], never from
//! herdr directly.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::contract::Error;
use crate::herdr::Herdr;
use crate::pty::{Kind, Spec, Terminal, Terminals};

/// The terminals and the panes of the sidebar of one daemon.
pub struct Sidebar {
    pub terminals: Arc<Terminals>,
    pub panes: Box<dyn PaneBackend>,
    /// The herdr of the herdr backend. The pty backend has none.
    pub herdr: Option<Herdr>,
    pub shell: Vec<String>,
    pub open_lock: Mutex<()>,
}

impl Sidebar {
    /// Open a terminal of `kind` (`shell` or `herdr`). There is one herdr client at a time:
    /// a second request gets the running one.
    pub fn open(&self, kind: &str) -> Result<Arc<Terminal>, Error> {
        let _one_open = self.open_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (kind, argv) = match (kind, &self.herdr) {
            ("shell", _) => (Kind::Shell, self.shell.clone()),
            ("herdr", Some(h)) => match self.terminals.running(Kind::Herdr) {
                Some(t) => return Ok(t),
                None => (Kind::Herdr, h.client_argv()),
            },
            ("herdr", None) => {
                return Err(Error::new(
                    409,
                    "term.herdr.absent",
                    "the pane backend is pty, so the sidebar has no herdr; install herdr and start fh serve with --panes herdr",
                ))
            }
            _ => return Err(Error::bad("term.kind", format!("unknown terminal kind {kind:?}; use shell or herdr"))),
        };
        self.terminals.open(Spec {
            kind,
            name: kind.name().into(),
            argv,
            cwd: None,
        })
    }
}

/// A pane ID: 1 to 64 letters, digits, `:`, `_` or `-`, with a letter or a digit first.
pub fn valid_pane(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-'))
}

/// A pane to open.
#[derive(Clone, Debug, Default)]
pub struct PaneSpec {
    pub title: String,
    /// The start folder. The default is the folder of the daemon.
    pub cwd: Option<PathBuf>,
    /// One command line to type into the shell of the pane.
    pub command: Option<String>,
}

/// A pane, for the text form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    /// `running`, `exited` or `closed`.
    pub state: String,
    /// The terminal of the daemon that shows the pane. A herdr pane has none: it shows in
    /// herdr, and herdr runs in the sidebar.
    pub terminal: Option<String>,
    pub title: String,
}

/// The state of the backend, for `GET /api/v1/panes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendStatus {
    /// `herdr` or `pty`.
    pub kind: &'static str,
    /// The herdr session, for the herdr backend.
    pub session: Option<String>,
    /// False when the herdr session is not running.
    pub running: bool,
    pub detail: String,
}

/// A key for [`PaneBackend::send`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Esc,
    Backspace,
    Up,
    Down,
    Right,
    Left,
    CtrlC,
    CtrlD,
}

impl Key {
    pub const ALL: [(&'static str, Key, &'static [u8]); 10] = [
        ("enter", Key::Enter, b"\r"),
        ("tab", Key::Tab, b"\t"),
        ("esc", Key::Esc, b"\x1b"),
        ("backspace", Key::Backspace, b"\x7f"),
        ("up", Key::Up, b"\x1b[A"),
        ("down", Key::Down, b"\x1b[B"),
        ("right", Key::Right, b"\x1b[C"),
        ("left", Key::Left, b"\x1b[D"),
        ("ctrl+c", Key::CtrlC, b"\x03"),
        ("ctrl+d", Key::CtrlD, b"\x04"),
    ];

    pub fn parse(name: &str) -> Result<Key, Error> {
        Key::ALL
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, k, _)| *k)
            .ok_or_else(|| {
                let names: Vec<_> = Key::ALL.iter().map(|(n, _, _)| *n).collect();
                Error::bad(
                    "pane.key",
                    format!("unknown key {name:?}; use one of {}", names.join(", ")),
                )
            })
    }

    pub fn name(self) -> &'static str {
        Key::ALL.iter().find(|(_, k, _)| *k == self).unwrap().0
    }

    fn bytes(self) -> &'static [u8] {
        Key::ALL.iter().find(|(_, k, _)| *k == self).unwrap().2
    }
}

/// The operations on panes. Both backends give the same errors: `pane.unknown` (404) for
/// a pane that does not exist or that femahoy does not own.
pub trait PaneBackend: Send + Sync {
    fn status(&self) -> BackendStatus;
    fn open(&self, spec: &PaneSpec) -> Result<Pane, Error>;
    fn list(&self) -> Result<Vec<Pane>, Error>;
    fn get(&self, id: &str) -> Result<Pane, Error>;
    /// The last `lines` lines of the pane as plain text.
    fn read(&self, id: &str, lines: usize) -> Result<String, Error>;
    /// Type `text`, then press `keys`.
    fn send(&self, id: &str, text: &str, keys: &[Key]) -> Result<(), Error>;
    fn close(&self, id: &str) -> Result<(), Error>;
}

/// Check a pane spec before a backend uses it.
pub fn check_spec(spec: &PaneSpec) -> Result<(), Error> {
    let t = &spec.title;
    if t.is_empty()
        || t.chars().count() > 64
        || t.chars().any(char::is_control)
        || t.starts_with('-')
    {
        return Err(Error::bad(
            "pane.title",
            "a pane title has 1 to 64 characters, no control character and no '-' first",
        ));
    }
    if let Some(cwd) = &spec.cwd {
        if !cwd.is_absolute() || !cwd.is_dir() {
            return Err(Error::bad(
                "pane.cwd",
                format!("{} is not an absolute path to a folder", cwd.display()),
            ));
        }
    }
    match &spec.command {
        Some(c) if c.len() > 4096 || c.chars().any(char::is_control) || c.starts_with('-') => {
            Err(Error::bad(
                "pane.command",
                "a pane command is one line of at most 4096 bytes and does not start with '-'",
            ))
        }
        _ => Ok(()),
    }
}

fn unknown(id: &str) -> Error {
    Error::new(404, "pane.unknown", format!("no pane {id:?}"))
}

/// The plain PTY backend: each pane is a shell in a terminal of the daemon. The pane ID is
/// the terminal ID, so the sidebar shows the pane as a terminal.
pub struct PtyPanes {
    pub terminals: Arc<Terminals>,
    pub shell: Vec<String>,
}

impl PtyPanes {
    fn terminal(&self, id: &str) -> Result<Arc<Terminal>, Error> {
        match self.terminals.get(id) {
            Ok(t) if t.kind == Kind::Pane => Ok(t),
            _ => Err(unknown(id)),
        }
    }

    fn pane(t: &Terminal) -> Pane {
        let info = t.info();
        Pane {
            id: info.id.clone(),
            state: info.state.into(),
            terminal: Some(info.id),
            title: info.title,
        }
    }
}

impl PaneBackend for PtyPanes {
    fn status(&self) -> BackendStatus {
        BackendStatus {
            kind: "pty",
            session: None,
            running: true,
            detail: "plain PTY panes owned by the daemon".into(),
        }
    }

    fn open(&self, spec: &PaneSpec) -> Result<Pane, Error> {
        check_spec(spec)?;
        let t = self.terminals.open(Spec {
            kind: Kind::Pane,
            name: spec.title.clone(),
            argv: self.shell.clone(),
            cwd: spec.cwd.clone(),
        })?;
        if let Some(command) = &spec.command {
            t.write(format!("{command}\r").as_bytes())?;
        }
        Ok(PtyPanes::pane(&t))
    }

    fn list(&self) -> Result<Vec<Pane>, Error> {
        Ok(self
            .terminals
            .list()
            .iter()
            .filter(|t| t.kind == Kind::Pane)
            .map(|t| PtyPanes::pane(t))
            .collect())
    }

    fn get(&self, id: &str) -> Result<Pane, Error> {
        Ok(PtyPanes::pane(&*self.terminal(id)?))
    }

    fn read(&self, id: &str, lines: usize) -> Result<String, Error> {
        Ok(self.terminal(id)?.text(lines))
    }

    fn send(&self, id: &str, text: &str, keys: &[Key]) -> Result<(), Error> {
        let t = self.terminal(id)?;
        let mut bytes = text.as_bytes().to_vec();
        keys.iter().for_each(|k| bytes.extend_from_slice(k.bytes()));
        t.write(&bytes)
    }

    fn close(&self, id: &str) -> Result<(), Error> {
        self.terminal(id)?;
        self.terminals.close(id)
    }
}
