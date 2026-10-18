//! The chat of the page (roadmap item M4, DESIGN.md section 5.1): the daemon runs the agent
//! as a Claude Code process in headless mode, with the login of the person (connection
//! path 1). femahoy has no agent loop of its own. The daemon reads the JSON event stream of
//! the agent (`--output-format stream-json`) and keeps each action as a chat entry: the
//! message, the text of the agent, each command, each tool and its result, and the end
//! with the time and the cost. When a run of the daemon succeeds during the chat, the chat
//! gets an `open` entry, and the page shows the results of that run. Before each solve,
//! `fh run` sends its input to the preview route, the chat gets a `show` entry, and the page
//! shows the model with each support and each load. When a run fails, the
//! chat gets a `failed` entry with the error.
//!
//! The agent process stays alive between the messages. Each message of the person goes to
//! its stdin as 1 line of `--input-format stream-json`, also while the agent works: the
//! agent reads it at its next step. The agent replays each message when it reads it
//! (`--replay-user-messages`). Thus the agent works while it has a message that it did not
//! read, or a turn with no `result`. A stop ends the agent and its processes. The next
//! message then starts a new agent that continues the session (`--resume`).
//!
//! The agent gets the instructions of `agent.md`, the state folder of the daemon in
//! `FH_STATE` (so `fh run` uses this daemon), `FH_NO_OPEN=1` and the folder of `fh` first
//! on `PATH`. It works in `STATE/chat`. Its tools are the `fh` commands, Python for the CAD
//! script, and the file tools. `FH_AGENT` names another agent program, for example a
//! fake agent that prints a recorded event stream in the tests.
//!
//! An entry is the item `<seq> <kind> <text>`. In the text, `\n` is a line break and `\\`
//! is a backslash, because an item is one line. The token of the daemon never goes into an
//! entry. An `error` entry is an error of the chat: the agent cannot start, or it ended
//! before the end of its work.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::contract::{now_ms, Error, Message, RunRecord, RunState};
use crate::json::{quote, Json};
use crate::service::Service;

/// The instructions of the agent.
const INSTRUCTIONS: &str = include_str!("agent.md");
/// The tools that the agent can use with no question to the person.
const TOOLS: &str = "Bash(fh *),Bash(fh-python *),Read,Write,Edit,Glob,Grep";
/// The most entries that the daemon keeps. An older entry goes first.
const MAX_ENTRIES: usize = 4000;
/// The most characters of one tool result in an entry.
const MAX_RESULT: usize = 4000;

#[derive(Default)]
struct Log {
    entries: VecDeque<(u64, String)>,
    next: u64,
    agent: Option<Child>,
    /// The stdin of the agent. Each message is 1 line on it.
    input: Option<ChildStdin>,
    /// The messages that went to the agent, and the messages that the agent read.
    sent: u64,
    taken: u64,
    /// The agent started a turn and did not end it with a `result`.
    turn: bool,
    /// The time when the agent started to work, in ms since the Unix epoch.
    started_ms: u64,
    /// The session of the agent, so that a new agent continues it.
    session: Option<String>,
    /// The runs that were final before the agent started to work.
    seen: HashSet<(String, String)>,
}

impl Log {
    /// The agent of the process `pid` is the current agent.
    fn current(&self, pid: u32) -> bool {
        self.agent.as_ref().is_some_and(|c| c.id() == pid)
    }

    /// The current agent is alive.
    fn alive(&mut self) -> bool {
        self.agent
            .as_mut()
            .is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    /// The agent works: it has a message that it did not read, or a turn with no end.
    fn running(&mut self) -> bool {
        (self.turn || self.sent > self.taken) && self.alive()
    }

    /// The agent works no more.
    fn rest(&mut self) {
        (self.sent, self.taken, self.turn) = (0, 0, false);
    }
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

    /// The entry `show <project> <revision> <title>`: the page shows the model of the
    /// revision with its supports and loads.
    pub(crate) fn show(&self, project: &str, revision: &str, title: &str) {
        let text = format!("{project} {revision} {title}");
        self.push(&mut self.lock(), "show", &text);
    }

    /// The state of the chat and the entries after `since`, as the items `<seq> <kind>
    /// <text>`. The fields are `state` (`running` or `idle`) and `next`. While the agent
    /// works, `started_ms` is the time when it started, and `solving` is the first run of
    /// the daemon that is not final: `<project> <run> <state> <last progress event>`.
    pub(crate) fn read(&self, since: u64) -> Message {
        let mut m = Message::new();
        let mut log = self.lock();
        let running = log.running();
        m.set("state", if running { "running" } else { "idle" });
        m.set("next", log.next);
        let items = log.entries.iter().filter(|(n, _)| *n > since);
        m.items = items.map(|(n, e)| format!("{n} {e}")).collect();
        if running {
            m.set("started_ms", log.started_ms);
            drop(log);
            if let Some((r, progress)) = self.service.active().into_iter().next() {
                let solving = format!("{} {} {} {progress}", r.project, r.run, r.state);
                m.set("solving", solving.trim_end());
            }
        }
        m
    }

    /// Send the message `text` of the person to the agent. If no agent is alive, start
    /// one first.
    pub(crate) fn ask(self: &Arc<Self>, text: &str) -> Result<(), Error> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::bad("chat.empty", "the message is empty"));
        }
        let mut log = self.lock();
        if !log.running() {
            log.rest();
            log.started_ms = now_ms();
            log.seen = self.finished();
        }
        self.push(&mut log, "you", text);
        if log.input.is_none() || !log.alive() {
            self.start(&mut log)?;
        }
        let line = format!(
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{}}},\"parent_tool_use_id\":null}}\n",
            quote(text)
        );
        let sent = match log.input.as_mut() {
            Some(input) => input
                .write_all(line.as_bytes())
                .and_then(|()| input.flush()),
            None => Err(std::io::ErrorKind::BrokenPipe.into()),
        };
        if let Err(e) = sent {
            let why = format!("cannot send the message to the agent: {e}");
            self.push(&mut log, "error", &why);
            log.input = None;
            return Err(Error::new(502, "chat.agent", why));
        }
        log.sent += 1;
        Ok(())
    }

    /// Start a new agent, which continues the session of the chat.
    fn start(self: &Arc<Self>, log: &mut Log) -> Result<(), Error> {
        let dir = self.state_dir.join("chat");
        std::fs::create_dir_all(&dir).map_err(|e| Error::io("chat folder", e))?;
        let program = std::env::var("FH_AGENT")
            .ok()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "claude".into());
        let mut cmd = Command::new(&program);
        // The messages go on stdin, so that no text of the person is read as an option.
        cmd.args(["-p", "--input-format", "stream-json"])
            .args(["--output-format", "stream-json", "--verbose"])
            .args([
                "--replay-user-messages",
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
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let why = format!("cannot start the agent {program:?}: {e}");
                self.push(log, "error", &why);
                return Err(Error::new(502, "chat.agent", why));
            }
        };
        let (out, err, pid) = (child.stdout.take(), child.stderr.take(), child.id());
        log.input = child.stdin.take();
        log.agent = Some(child);
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
            std::thread::spawn(move || chat.follow(out, pid));
        }
        Ok(())
    }

    /// Stop the agent and each process that it started.
    pub(crate) fn stop(&self) {
        let mut log = self.lock();
        let running = log.running();
        log.rest();
        log.input = None;
        let Some(mut child) = log.agent.take() else {
            return;
        };
        // The agent is the first process of its own group.
        #[cfg(unix)]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGTERM);
        }
        #[cfg(not(unix))]
        let _ = child.kill();
        std::thread::spawn(move || child.wait());
        if running {
            self.push(&mut log, "note", "stopped the agent");
        }
    }

    /// The final runs of the daemon, with their records.
    fn final_runs(&self) -> Vec<RunRecord> {
        let projects = self.service.projects().into_iter();
        let runs = projects.flat_map(|(p, _)| self.service.runs(&p).unwrap_or_default());
        runs.filter(|r| r.state.is_final()).collect()
    }

    /// The keys of the final runs of the daemon.
    fn finished(&self) -> HashSet<(String, String)> {
        let runs = self.final_runs().into_iter();
        runs.map(|r| (r.project, r.run)).collect()
    }

    /// Add an entry for each run that became final since the agent started to work:
    /// `open <project> <run>` for a run that succeeded, `failed <project> <run> <error>`
    /// for a run that failed.
    fn report_runs(&self) {
        let mut runs = self.final_runs();
        runs.sort_by(|a, b| (&a.project, &a.run).cmp(&(&b.project, &b.run)));
        let mut log = self.lock();
        for r in runs {
            if !log.seen.insert((r.project.clone(), r.run.clone())) {
                continue;
            }
            let (project, run) = (&r.project, &r.run);
            match r.state {
                RunState::Succeeded => self.push(&mut log, "open", &format!("{project} {run}")),
                RunState::Failed => {
                    let text = format!("{project} {run} {}", r.message);
                    self.push(&mut log, "failed", &text);
                }
                _ => {}
            }
        }
    }

    /// Read the event stream of the agent until it ends.
    fn follow(&self, out: impl Read, pid: u32) {
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
            let ended = entries.iter().any(|(k, _)| k == "end");
            if ended {
                // A run can end at the last step of the agent.
                self.report_runs();
            }
            let mut log = self.lock();
            if let Some(s) = event.text(&["session_id"]) {
                log.session = Some(s.to_owned());
            }
            if log.current(pid) {
                if event.flag(&["isReplay"]) == Some(true) {
                    log.taken += 1;
                    log.turn = true;
                }
                log.turn |= event.text(&["type"]) == Some("assistant");
                if ended {
                    log.turn = false;
                }
            }
            for (kind, text) in &entries {
                self.push(&mut log, kind, text);
            }
            drop(log);
            if after_tool {
                self.report_runs();
            }
        }
        // Wait for the exit with no lock, so that a read of the chat does not wait.
        let code = loop {
            let mut log = self.lock();
            if !log.current(pid) {
                // A stop or a new agent took this agent.
                return;
            }
            match log.agent.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                Some(Ok(Some(code))) => break code.code(),
                _ => break None,
            }
            drop(log);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let mut log = self.lock();
        log.input = None;
        if log.turn || log.sent > log.taken {
            log.rest();
            let why = format!("the agent ended before the end of its work (exit code {code:?})");
            self.push(&mut log, "error", &why);
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
