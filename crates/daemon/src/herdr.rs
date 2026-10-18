//! The herdr pane backend (DESIGN.md section 4.3). femahoy never bundles herdr: it calls
//! the `herdr` program that the user installed.
//!
//! Every call is `herdr --session <session> ...` on the femahoy session, never `default`. Only the
//! commands in [`ALLOWED`] run, so femahoy never starts or stops a server. A call gets no `HERDR_*`
//! variable of the daemon. femahoy uses one workspace, `femahoy`, and only its panes.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::contract::Error;
use crate::json::Json;
use crate::panes::{check_spec, BackendStatus, Key, Pane, PaneBackend, PaneSpec};
use crate::pty::env_names;

pub const WORKSPACE: &str = "femahoy";
pub const TIMEOUT: Duration = Duration::from_secs(10);

pub const ALLOWED: [&str; 12] = [
    "status --json",
    "workspace list",
    "workspace create",
    "tab create",
    "pane list",
    "pane get",
    "pane read",
    "pane rename",
    "pane run",
    "pane send-text",
    "pane send-keys",
    "pane close",
];

pub fn valid_session(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(c))
        && !name.eq_ignore_ascii_case("default")
}

pub fn check_session(session: &str) -> Result<(), Error> {
    if valid_session(session) {
        return Ok(());
    }
    let why = "use letters, digits, '_' and '-', and never 'default'";
    let message = format!("{session:?} is not a herdr session that femahoy can own: {why}");
    Err(Error::bad("pane.herdr.session", message))
}

#[derive(Clone, Debug)]
pub struct Herdr {
    program: PathBuf,
    session: String,
    open_lock: Arc<Mutex<()>>,
}

impl Herdr {
    pub fn new(program: PathBuf, session: &str) -> Result<Herdr, Error> {
        check_session(session)?;
        Ok(Herdr {
            program,
            session: session.into(),
            open_lock: Arc::default(),
        })
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    pub fn client_argv(&self) -> Vec<String> {
        vec![
            self.program.display().to_string(),
            "--session".into(),
            self.session.clone(),
        ]
    }

    pub fn call(&self, args: &[&str]) -> Result<String, Error> {
        let head = args.iter().take(2).copied().collect::<Vec<_>>().join(" ");
        if !ALLOWED.contains(&head.as_str()) {
            return Err(Error::new(
                500,
                "pane.herdr.refused",
                format!("femahoy does not run 'herdr {head}'"),
            ));
        }
        let mut cmd = Command::new(&self.program);
        cmd.arg("--session").arg(&self.session).args(args);
        for k in env_names().filter(|k| k.starts_with("HERDR_")) {
            cmd.env_remove(k);
        }
        cmd.env("HERDR_SESSION", &self.session)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let program = self.program.display();
        let child = cmd
            .spawn()
            .map_err(|e| Error::new(502, "pane.herdr", format!("cannot start {program}: {e}")))?;
        let pid = child.id() as i32;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let out = match rx.recv_timeout(TIMEOUT) {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(Error::new(502, "pane.herdr", e.to_string())),
            Err(_) => {
                // The thread reaps the child, so the pid is still the child here.
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                let _ = pid;
                let message = format!("'herdr {head}' took longer than {TIMEOUT:?}");
                return Err(Error::new(504, "pane.herdr.timeout", message));
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            return Ok(stdout);
        }
        // herdr 0.9 writes an error as JSON on stderr. Also look at stdout.
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        let json = [stderr.as_str(), stdout.trim()]
            .into_iter()
            .find_map(|text| {
                Json::parse(text)
                    .ok()
                    .filter(|j| j.at(&["error"]).is_some())
            });
        let code = json.as_ref().and_then(|j| j.text(&["error", "code"]));
        let message = json
            .as_ref()
            .and_then(|j| j.text(&["error", "message"]))
            .map(str::to_owned)
            .unwrap_or(stderr);
        Err(match code {
            Some("pane_not_found" | "workspace_not_found" | "tab_not_found") => {
                Error::new(404, "pane.unknown", message)
            }
            _ => Error::new(502, "pane.herdr", format!("'herdr {head}': {message}")),
        })
    }

    fn json(&self, args: &[&str]) -> Result<Json, Error> {
        let out = self.call(args)?;
        Json::parse(out.trim())
            .map_err(|e| Error::new(502, "pane.herdr", format!("herdr gave bad JSON: {e}")))
    }

    pub fn status(&self) -> Result<(String, bool), Error> {
        let j = self.json(&["status", "--json"])?;
        let version = j
            .text(&["client", "version"])
            .unwrap_or("unknown")
            .to_owned();
        Ok((version, j.flag(&["server", "running"]) == Some(true)))
    }
}

pub struct HerdrPanes(pub Herdr);

fn pane_of(j: &Json) -> Pane {
    let id = j.text(&["pane_id"]).unwrap_or("").to_owned();
    Pane {
        title: j.text(&["label"]).unwrap_or(&id).to_owned(),
        id,
        state: "running".into(),
        terminal: None,
    }
}

impl HerdrPanes {
    fn running(&self) -> Result<(), Error> {
        if self.0.status()?.1 {
            return Ok(());
        }
        let s = self.0.session();
        let why = "open Herdr in the terminal sidebar to start it";
        let message = format!("the herdr session {s:?} is not running: {why}");
        Err(Error::new(409, "pane.herdr.stopped", message))
    }

    fn workspace(&self) -> Result<Option<String>, Error> {
        let j = self.0.json(&["workspace", "list"])?;
        let list = j.list(&["result", "workspaces"]);
        let ws = list.iter().find(|w| w.text(&["label"]) == Some(WORKSPACE));
        Ok(ws
            .and_then(|w| w.text(&["workspace_id"]))
            .map(str::to_owned))
    }

    fn owned(&self, id: &str) -> Result<Json, Error> {
        self.running()?;
        let j = self.0.json(&["pane", "get", id])?;
        let pane = j.at(&["result", "pane"]).cloned().unwrap_or(Json::Null);
        match (pane.text(&["workspace_id"]), self.workspace()?) {
            (Some(w), Some(ours)) if w == ours => Ok(pane),
            _ => {
                let message = format!("no pane {id:?} in the femahoy workspace");
                Err(Error::new(404, "pane.unknown", message))
            }
        }
    }
}

impl PaneBackend for HerdrPanes {
    fn status(&self) -> BackendStatus {
        let (running, detail) = match self.0.status() {
            Ok((version, running)) => (running, format!("herdr {version}")),
            Err(e) => (false, e.message),
        };
        let session = Some(self.0.session().to_owned());
        BackendStatus {
            kind: "herdr",
            session,
            running,
            detail,
        }
    }

    /// The first pane is the root pane of a new workspace; later panes are new tabs in it.
    fn open(&self, spec: &PaneSpec) -> Result<Pane, Error> {
        check_spec(spec)?;
        let _one_open = self.0.open_lock.lock().unwrap_or_else(|e| e.into_inner());
        self.running()?;
        let cwd = match &spec.cwd {
            Some(c) => c.clone(),
            None => std::env::current_dir().map_err(|e| Error::io("the daemon folder", e))?,
        };
        let (cwd, title, ws) = (
            cwd.display().to_string(),
            spec.title.as_str(),
            self.workspace()?,
        );
        let mut args = match &ws {
            Some(ws) => vec!["tab", "create", "--workspace", ws, "--label", title],
            None => vec!["workspace", "create", "--label", WORKSPACE],
        };
        args.extend(["--cwd", &cwd, "--no-focus"]);
        let created = self.0.json(&args)?;
        let id = created.text(&["result", "root_pane", "pane_id"]);
        let id = id.ok_or_else(|| Error::new(502, "pane.herdr", "herdr gave no pane ID"))?;
        self.0.call(&["pane", "rename", id, title])?;
        if let Some(command) = &spec.command {
            self.0.call(&["pane", "run", id, command])?;
        }
        Ok(Pane {
            id: id.into(),
            state: "running".into(),
            terminal: None,
            title: title.into(),
        })
    }

    fn list(&self) -> Result<Vec<Pane>, Error> {
        self.running()?;
        let Some(ws) = self.workspace()? else {
            return Ok(Vec::new());
        };
        let j = self.0.json(&["pane", "list", "--workspace", &ws])?;
        Ok(j.list(&["result", "panes"]).iter().map(pane_of).collect())
    }

    fn get(&self, id: &str) -> Result<Pane, Error> {
        Ok(pane_of(&self.owned(id)?))
    }

    fn read(&self, id: &str, lines: usize) -> Result<String, Error> {
        self.owned(id)?;
        let n = lines.to_string();
        let args = [
            "pane",
            "read",
            id,
            "--source",
            "recent-unwrapped",
            "--lines",
            &n,
        ];
        Ok(self.0.call(&args)?.trim_end().to_owned())
    }

    fn send(&self, id: &str, text: &str, keys: &[Key]) -> Result<(), Error> {
        self.owned(id)?;
        if !text.is_empty() {
            self.0.call(&["pane", "send-text", id, text])?;
        }
        if !keys.is_empty() {
            let mut args = vec!["pane", "send-keys", id];
            args.extend(keys.iter().map(|k| k.name()));
            self.0.call(&args)?;
        }
        Ok(())
    }

    fn close(&self, id: &str) -> Result<(), Error> {
        self.owned(id)?;
        self.0.call(&["pane", "close", id]).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names() {
        for good in ["femahoy", "fm-lab-femahoy-1_2", "a"] {
            assert!(valid_session(good), "{good}");
        }
        let long = "a".repeat(65);
        for bad in ["", "default", "DEFAULT", "-x", "a b", "a/b", "a.b", &long] {
            assert!(!valid_session(bad), "{bad}");
        }
        assert_eq!(
            Herdr::new("herdr".into(), "default").unwrap_err().code,
            "pane.herdr.session"
        );
    }

    #[test]
    fn only_the_allowed_commands_run() {
        let h = Herdr::new("false".into(), "femahoy").unwrap();
        for args in [
            &["server", "stop"][..],
            &["session", "stop"],
            &["session", "delete"],
            &["update"],
            &["status"],
        ] {
            assert_eq!(
                h.call(args).unwrap_err().code,
                "pane.herdr.refused",
                "{args:?}"
            );
        }
    }
}
