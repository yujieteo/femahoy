//! The chat of the page (roadmap item M4, DESIGN.md section 5.1): the daemon runs the agent
//! as a Claude Code process in headless mode, with the login of the person (connection
//! path 1). femahoy has no agent loop of its own. The daemon reads the JSON event stream of
//! the agent (`--output-format stream-json`) and keeps each action as a chat entry: the
//! request, the text of the agent, each command, each tool and its result, and the end
//! with the time and the cost. When a run of the daemon succeeds during the chat, the chat
//! gets an `open` entry, and the page shows the results of that run.
//!
//! The agent gets the instructions of `agent.md`, the state folder of the daemon in
//! `FH_STATE` (so `fh run` uses this daemon), `FH_NO_OPEN=1` and the folder of `fh` first
//! on `PATH`. It works in `STATE/chat`. Its tools are the `fh` commands, Python for the CAD
//! script, and the file tools. `FH_AGENT` names another agent program, for example a
//! fake agent that prints a recorded event stream in the tests.
//!
//! An entry is the item `<seq> <kind> <text>`. In the text, `\n` is a line break and `\\`
//! is a backslash, because an item is one line. The token of the daemon never goes into an
//! entry.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::contract::{Error, RunState};
use crate::json::Json;
use crate::service::Service;

/// The instructions of the agent.
const INSTRUCTIONS: &str = include_str!("agent.md");
/// The tools that the agent can use with no question to the person.
const TOOLS: &str = "Bash(fh *),Bash(python3 *),Bash(uv run *),Read,Write,Edit,Glob,Grep";
/// The most entries that the daemon keeps. An older entry goes first.
const MAX_ENTRIES: usize = 4000;
/// The most characters of one tool result in an entry.
const MAX_RESULT: usize = 4000;

#[derive(Default)]
struct Log {
    entries: VecDeque<(u64, String)>,
    next: u64,
    agent: Option<Child>,
    /// The session of the agent, so that the next request continues it.
    session: Option<String>,
    /// The runs that succeeded before this point of the chat.
    seen: HashSet<(String, String)>,
}

pub(crate) struct Chat {
    service: Arc<Service>,
    state_dir: PathBuf,
    token: String,
    log: Mutex<Log>,
}

/// The text of an entry on one line.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "")
}

impl Chat {
    pub(crate) fn new(service: Arc<Service>, state_dir: PathBuf, token: String) -> Arc<Chat> {
        Arc::new(Chat {
            service,
            state_dir,
            token,
            log: Mutex::default(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn push(&self, log: &mut Log, kind: &str, text: &str) {
        let text = escape(&text.replace(&self.token, "<token>"));
        log.next += 1;
        log.entries.push_back((log.next, format!("{kind} {text}")));
        if log.entries.len() > MAX_ENTRIES {
            log.entries.pop_front();
        }
    }

    /// `running` while the agent works, else `idle`.
    fn running(log: &mut Log) -> bool {
        log.agent
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// The state and the entries after `since`, as the items `<seq> <kind> <text>`.
    pub(crate) fn read(&self, since: u64) -> (&'static str, u64, Vec<String>) {
        let mut log = self.lock();
        let state = if Chat::running(&mut log) {
            "running"
        } else {
            "idle"
        };
        let items = log.entries.iter().filter(|(n, _)| *n > since);
        let items = items.map(|(n, e)| format!("{n} {e}")).collect();
        (state, log.next, items)
    }

    /// Start the agent on the request `text` of the person.
    pub(crate) fn ask(self: &Arc<Self>, text: &str) -> Result<(), Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::bad("chat.empty", "the request is empty"));
        }
        let mut log = self.lock();
        if Chat::running(&mut log) {
            return Err(Error::new(409, "chat.busy", "the agent still works"));
        }
        let dir = self.state_dir.join("chat");
        std::fs::create_dir_all(&dir).map_err(|e| Error::io("chat folder", e))?;
        let program = std::env::var("FH_AGENT")
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "claude".into());
        let mut cmd = Command::new(&program);
        // The request goes on stdin, so that no text of the person is read as an option.
        cmd.args(["-p", "--output-format", "stream-json", "--verbose"])
            .args([
                "--append-system-prompt",
                INSTRUCTIONS,
                "--allowedTools",
                TOOLS,
            ]);
        if let Some(session) = &log.session {
            cmd.args(["--resume", session]);
        }
        let mut path = std::env::current_exe()
            .ok()
            .and_then(|e| Some(vec![e.parent()?.to_path_buf()]))
            .unwrap_or_default();
        path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        cmd.current_dir(&dir)
            .env("FH_STATE", &self.state_dir)
            .env("FH_NO_OPEN", "1")
            .env("PATH", std::env::join_paths(path).unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        self.push(&mut log, "you", text);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let why = format!("cannot start the agent {program:?}: {e}");
                self.push(&mut log, "fail", &why);
                return Err(Error::new(502, "chat.agent", why));
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            // The drop closes stdin, and the agent starts.
            let _ = stdin.write_all(text.as_bytes());
        }
        let (out, err) = (child.stdout.take(), child.stderr.take());
        log.agent = Some(child);
        log.seen = self.succeeded();
        drop(log);
        if let Some(err) = err {
            let chat = self.clone();
            std::thread::spawn(move || {
                let mut last = VecDeque::new();
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    last.push_back(line);
                    if last.len() > 40 {
                        last.pop_front();
                    }
                }
                let text = Vec::from(last).join("\n");
                if !text.trim().is_empty() {
                    chat.push(&mut chat.lock(), "note", text.trim());
                }
            });
        }
        if let Some(out) = out {
            let chat = self.clone();
            std::thread::spawn(move || chat.follow(out));
        }
        Ok(())
    }

    /// Stop the agent and each process that it started.
    pub(crate) fn stop(&self) {
        let mut log = self.lock();
        if !Chat::running(&mut log) {
            return;
        }
        if let Some(child) = log.agent.as_mut() {
            // The agent is the first process of its own group.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
        }
        self.push(&mut log, "note", "stopped the agent");
    }

    /// The succeeded runs of the daemon.
    fn succeeded(&self) -> HashSet<(String, String)> {
        let mut runs = HashSet::new();
        for (project, _) in self.service.projects() {
            for r in self.service.runs(&project).unwrap_or_default() {
                if r.state == RunState::Succeeded {
                    runs.insert((project.clone(), r.run));
                }
            }
        }
        runs
    }

    /// Read the event stream of the agent until it ends.
    fn follow(&self, out: impl Read) {
        let mut ended = false;
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { break };
            let Ok(event) = Json::parse(&line) else {
                if !line.trim().is_empty() {
                    self.push(&mut self.lock(), "note", &line);
                }
                continue;
            };
            let entries = entries(&event);
            let after_tool = entries.iter().any(|(k, _)| k == "out" || k == "fail");
            ended |= entries.iter().any(|(k, _)| k == "end");
            let mut log = self.lock();
            if let Some(s) = event.text(&["session_id"]) {
                log.session = Some(s.to_owned());
            }
            for (kind, text) in &entries {
                self.push(&mut log, kind, text);
            }
            if after_tool {
                drop(log);
                let now = self.succeeded();
                let mut log = self.lock();
                let mut new: Vec<_> = now.difference(&log.seen).cloned().collect();
                new.sort();
                for (p, r) in new {
                    self.push(&mut log, "open", &format!("{p} {r}"));
                }
                log.seen = now;
            }
        }
        // Wait for the exit with no lock, so that a read of the chat does not wait.
        let code = loop {
            let mut log = self.lock();
            match log.agent.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                Some(Ok(Some(code))) => break Some(code),
                _ => break None,
            }
            drop(log);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let mut log = self.lock();
        if !ended {
            let code = code.and_then(|c| c.code());
            let why = format!("the agent ended with no result (exit code {code:?})");
            self.push(&mut log, "fail", &why);
        }
    }
}

/// The chat entries of one event of the stream: `(kind, text)`.
///
/// | Event | Entries |
/// |---|---|
/// | `system`, subtype `init` | `note`: the model and the folder |
/// | `assistant` | `say` for a text, `cmd` for a `Bash` command, `tool` for another tool |
/// | `user` | `out` for a tool result, `fail` for a tool error |
/// | `result` | `end`: the outcome, the wall time, the turns and the cost |
fn entries(event: &Json) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |kind: &str, text: String| out.push((kind.to_owned(), text));
    let number = |path: &[&str]| match event.at(path) {
        Some(Json::Number(n)) => n.parse::<f64>().ok(),
        _ => None,
    };
    match event.text(&["type"]).unwrap_or("") {
        "system" if event.text(&["subtype"]) == Some("init") => add(
            "note",
            format!(
                "agent {} in {}",
                event.text(&["model"]).unwrap_or("?"),
                event.text(&["cwd"]).unwrap_or("?")
            ),
        ),
        "assistant" => {
            for block in event.list(&["message", "content"]) {
                match block.text(&["type"]).unwrap_or("") {
                    "text" => add("say", block.text(&["text"]).unwrap_or("").to_owned()),
                    "tool_use" => match block.text(&["name"]).unwrap_or("?") {
                        "Bash" => add(
                            "cmd",
                            block.text(&["input", "command"]).unwrap_or("").to_owned(),
                        ),
                        name => {
                            let input = ["file_path", "pattern", "path"]
                                .iter()
                                .find_map(|k| block.text(&["input", k]))
                                .unwrap_or("");
                            add("tool", format!("{name} {input}").trim_end().to_owned())
                        }
                    },
                    _ => {}
                }
            }
        }
        "user" => {
            for block in event.list(&["message", "content"]) {
                if block.text(&["type"]) != Some("tool_result") {
                    continue;
                }
                let mut text = match block.at(&["content"]) {
                    Some(Json::Text(t)) => t.clone(),
                    Some(Json::List(parts)) => parts
                        .iter()
                        .filter_map(|p| p.text(&["text"]))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                if text.chars().count() > MAX_RESULT {
                    let cut: String = text.chars().take(MAX_RESULT).collect();
                    text = format!("{cut}\n... (the result is longer)");
                }
                let error = block.flag(&["is_error"]) == Some(true);
                add(if error { "fail" } else { "out" }, text);
            }
        }
        "result" => {
            let outcome = match (event.text(&["subtype"]), event.flag(&["is_error"])) {
                (Some("success"), Some(false) | None) => "done".to_owned(),
                (subtype, _) => format!("failed ({})", subtype.unwrap_or("error")),
            };
            let seconds = number(&["duration_ms"]).unwrap_or(0.0) / 1000.0;
            let turns = number(&["num_turns"]).unwrap_or(0.0);
            let cost = number(&["total_cost_usd"]).unwrap_or(0.0);
            add(
                "end",
                format!("{outcome} in {seconds:.1} s, {turns} turns, cost {cost:.4} USD"),
            );
        }
        _ => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, String)> {
        entries(&Json::parse(line).unwrap())
    }

    #[test]
    fn each_action_of_the_agent_is_an_entry() {
        let cmd = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"I mesh it."},{"type":"tool_use","name":"Bash","input":{"command":"fh run . --size 2.5"}},{"type":"tool_use","name":"Write","input":{"file_path":"part.py","content":"x"}}]}}"#;
        let pair = |k: &str, t: &str| (k.to_owned(), t.to_owned());
        assert_eq!(
            kinds(cmd),
            [
                pair("say", "I mesh it."),
                pair("cmd", "fh run . --size 2.5"),
                pair("tool", "Write part.py"),
            ]
        );
        let out = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"ok\tgmsh","is_error":false},{"type":"tool_result","content":[{"type":"text","text":"fh mesh: mesh.region.missing"}],"is_error":true}]}}"#;
        assert_eq!(
            kinds(out),
            [
                pair("out", "ok\tgmsh"),
                pair("fail", "fh mesh: mesh.region.missing")
            ]
        );
        let end = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":81234,"num_turns":9,"total_cost_usd":0.41237}"#;
        assert_eq!(
            kinds(end),
            [pair("end", "done in 81.2 s, 9 turns, cost 0.4124 USD")]
        );
        let init = r#"{"type":"system","subtype":"init","cwd":"/s/chat","model":"m"}"#;
        assert_eq!(kinds(init), [pair("note", "agent m in /s/chat")]);
        let hook = r#"{"type":"system","subtype":"hook_started"}"#;
        assert!(kinds(hook).is_empty());
    }

    #[test]
    fn an_entry_is_one_line() {
        assert_eq!(escape("a\\b\r\nc"), "a\\\\b\\nc");
    }
}
