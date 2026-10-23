//! The chat of the page (DESIGN.md section 4.3): the daemon runs the agent
//! as a Claude Code process in headless mode, with the login of the person (connection
//! path 1). femahoy has no agent loop of its own. The daemon reads the JSON event stream of
//! the agent (`--output-format stream-json`) and keeps each action as a chat entry: the
//! message, the text of the agent, each command, each tool and its result, and the end
//! with the time and the cost. The daemon also reads the state folder (`store` module). Before
//! each solve, `fh run` writes a preview folder, the chat gets a `show` entry, and the page
//! shows the model with each support and each load. When a run gets its results, the chat
//! gets an `open` entry, and the page shows the results of that run.
//!
//! The agent process stays alive between the messages. Each message of the person goes to
//! its stdin as 1 line of `--input-format stream-json`, also while the agent works: the
//! agent reads it at its next step. The agent replays each message when it reads it
//! (`--replay-user-messages`). Thus the agent works while it has a message that it did not
//! read, or a turn with no `result`. A stop ends the agent and its processes. The next
//! message then starts a new agent that continues the session (`--resume`).
//!
//! The agent gets the instructions of `agent.md`, the state folder of the daemon in
//! `FH_STATE` (so `fh run` writes its runs where this daemon reads them) and the folder of
//! `fh` first on `PATH`. It ignores the personal Claude Code setup of the person: the user
//! settings with their hooks and plugins, `~/.claude/CLAUDE.md`, the MCP servers and the
//! claude.ai connectors. It loads no setting source at all, because Claude Code reads the
//! settings and the `CLAUDE.md` files of each folder above `STATE/chat` as project files, also
//! `~/.claude/CLAUDE.md` when the state folder is in the home folder. Thus each person gets the
//! same agent, with the default model and the login of the person.
//!
//! The agent works in `STATE/chat`: it can make, change and delete the files there with the file
//! tools and with file commands such as `mkdir`, `mv`, `rm` and an output redirect
//! (`--permission-mode acceptEdits`). Its other commands are `fh` and `fh-python` (the Python
//! of the CAD script). Solves go through `fh run`. `FH_AGENT` names
//! another agent program, for example a fake agent that prints a recorded event stream in the
//! tests.
//!
//! An entry is the item `<seq> <kind> <text>`. In the text, `\n` is a line break and `\\`
//! is a backslash, because an item is one line. The token of the daemon never goes into an
//! entry. An `error` entry is an error of the chat: the agent cannot start, or it ended
//! before the end of its work. A `setup` entry tells the person what to do before the chat
//! works: install Claude Code, or sign in (`agent_problem`, which `fh serve` runs at start).

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use super::json::{quote, Json};
use super::message::{bad, fail, Failure, Message};
use super::store::Store;

/// The instructions of the agent.
const INSTRUCTIONS: &str = include_str!("agent.md");
/// The tools that the agent can use with no question to the person.
const TOOLS: &str = "Bash(fh *),Bash(fh-python *),Read,Write,Edit,\
                     Glob,Grep,WebSearch,WebFetch";
/// The Bash time limit of the agent in ms: 24 hours. Claude Code stops a Bash command after
/// 2 minutes by default, and a solve of a large model can take hours.
const BASH_TIMEOUT_MS: &str = "86400000";
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
    /// The daemon reads the event stream of the current agent. It stops at the exit of the
    /// agent, after the last entry of the agent.
    reading: bool,
    /// The time when the agent started to work, in ms since the Unix epoch.
    started_ms: u64,
    /// The session of the agent, so that a new agent continues it.
    session: Option<String>,
    /// The previews that the chat shows, and the runs that it opens.
    previews: HashSet<String>,
    runs: HashSet<String>,
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
    fn running(&self) -> bool {
        self.reading && (self.turn || self.sent > self.taken)
    }

    /// The agent works no more.
    fn rest(&mut self) {
        (self.sent, self.taken, self.turn) = (0, 0, false);
    }

    /// Stop the current agent and each process that it started, and forget its work.
    fn end(&mut self) {
        self.rest();
        (self.input, self.reading) = (None, false);
        let Some(mut child) = self.agent.take() else {
            return;
        };
        // The agent is the first process of its own group. SIGTERM is 15 on Linux and macOS.
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        unsafe {
            kill(-(child.id() as i32), 15);
        }
        std::thread::spawn(move || child.wait());
    }
}

pub(crate) struct Chat {
    store: Arc<Store>,
    state_dir: PathBuf,
    token: String,
    log: Mutex<Log>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The text of an entry on one line.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "")
}

impl Chat {
    /// The runs that have results and the previews that the state folder holds already do not
    /// get an entry.
    pub(crate) fn new(store: Arc<Store>, state_dir: PathBuf, token: String) -> Arc<Chat> {
        let old = store.runs().unwrap_or_default();
        let shown = store.previews();
        let log = Log {
            runs: old.into_iter().map(|(run, _)| run).collect(),
            previews: shown.into_iter().map(|(preview, _)| preview).collect(),
            ..Log::default()
        };
        Arc::new(Chat {
            store,
            state_dir,
            token,
            log: Mutex::new(log),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Add the entry `setup <text>`: what the person must do before the chat works.
    pub(crate) fn setup(&self, text: &str) {
        let mut log = self.lock();
        self.push(&mut log, "setup", text);
    }

    fn push(&self, log: &mut Log, kind: &str, text: &str) {
        let text = escape(&text.replace(&self.token, "<token>"));
        log.next += 1;
        log.entries.push_back((log.next, format!("{kind} {text}")));
        if log.entries.len() > MAX_ENTRIES {
            log.entries.pop_front();
        }
    }

    /// The state of the chat and the entries after `since`, as the items `<seq> <kind>
    /// <text>`. The fields are `state` (`running` or `idle`) and `next`. While the agent
    /// works, `started_ms` is the time when it started.
    pub(crate) fn read(&self, since: u64) -> Message {
        self.report();
        let mut m = Message::new();
        let log = self.lock();
        let running = log.running();
        m.set("state", if running { "running" } else { "idle" });
        m.set("next", log.next);
        let items = log.entries.iter().filter(|(n, _)| *n > since);
        m.items = items.map(|(n, e)| format!("{n} {e}")).collect();
        if running {
            m.set("started_ms", log.started_ms);
        }
        m
    }

    /// Send the message `text` of the person to the agent. If no agent is alive, start
    /// one first.
    pub(crate) fn ask(self: &Arc<Self>, text: &str) -> Result<(), Failure> {
        let text = text.trim();
        if text.is_empty() {
            return Err(bad("the message is empty"));
        }
        let mut log = self.lock();
        if log.input.is_none() || !log.alive() {
            self.start(&mut log)?;
        }
        if !log.running() {
            log.rest();
            log.started_ms = now_ms();
        }
        self.push(&mut log, "you", text);
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
            return Err(fail(502, why));
        }
        log.sent += 1;
        Ok(())
    }

    /// Start a new agent, which continues the session of the chat.
    fn start(self: &Arc<Self>, log: &mut Log) -> Result<(), Failure> {
        if log.running() {
            // The agent ended, and the daemon did not see the end of its stream yet.
            self.push(log, "error", "the agent ended before the end of its work");
        }
        log.end();
        let dir = self.state_dir.join("chat");
        std::fs::create_dir_all(&dir).map_err(|e| fail(500, format!("chat folder: {e}")))?;
        let program = agent_program();
        let mut cmd = agent_command(&program, &self.state_dir, log.session.as_deref());
        cmd.current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                let why = format!("cannot start the agent {program:?}: {e}");
                self.push(log, "error", &why);
                return Err(fail(502, why));
            }
        };
        let (out, err, pid) = (child.stdout.take(), child.stderr.take(), child.id());
        (log.input, log.reading) = (child.stdin.take(), true);
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
        log.end();
        if running {
            self.push(&mut log, "note", "stopped the agent");
        }
    }

    /// Add an entry for each new preview, `show <preview> <title>`, and for each run that has
    /// new results, `open <run>`.
    fn report(&self) {
        let previews = self.store.previews();
        let runs = self.store.runs().unwrap_or_default();
        let mut log = self.lock();
        for (preview, title) in previews {
            if log.previews.insert(preview.clone()) {
                self.push(&mut log, "show", &format!("{preview} {title}"));
            }
        }
        for (run, _) in runs {
            if log.runs.insert(run.clone()) {
                self.push(&mut log, "open", &run);
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
                self.report();
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
                self.report();
            }
        }
        // Wait for the exit with no lock, so that a read of the chat does not wait.
        let (mut log, code) = loop {
            let mut log = self.lock();
            if !log.current(pid) {
                // A stop or a new agent took this agent.
                return;
            }
            match log.agent.as_mut().map(Child::try_wait) {
                Some(Ok(None)) => {}
                Some(Ok(Some(code))) => break (log, code.code()),
                _ => break (log, None),
            }
            drop(log);
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        if log.running() {
            let why = format!("the agent ended before the end of its work (exit code {code:?})");
            self.push(&mut log, "error", &why);
        }
        log.rest();
        (log.input, log.reading) = (None, false);
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

/// The agent program: `$FH_AGENT`, else `claude`.
pub(crate) fn agent_program() -> String {
    std::env::var("FH_AGENT")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "claude".into())
}

/// What the person must do before the chat works, or `None`. The agent `program` must start, and
/// `program auth status` must end with exit code 0: Claude Code then has a login. The login stays
/// in Claude Code, and the daemon reads no credential.
pub(crate) fn agent_problem(program: &str) -> Option<String> {
    let status = Command::new(program)
        .args(["auth", "status"])
        .env("PATH", agent_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(status) if status.success() => None,
        Ok(_) => Some(format!(
            "Claude Code ({program}) has no login. In a terminal, run {program} and sign in with \
             your Claude subscription. Then send your message."
        )),
        Err(e) => Some(format!(
            "The chat needs Claude Code, and the agent {program:?} does not start: {e}. Install \
             Claude Code in a terminal:\n  curl -fsSL https://claude.ai/install.sh | bash\nThen \
             run claude and sign in with your Claude subscription. Then send your message."
        )),
    }
}

/// The `PATH` of the agent: the folder of `fh` first, so that the agent runs this `fh`.
fn agent_path() -> std::ffi::OsString {
    let mut path = std::env::current_exe()
        .ok()
        .and_then(|e| Some(vec![e.parent()?.to_path_buf()]))
        .unwrap_or_default();
    path.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(path).unwrap_or_default()
}

/// The agent command with its arguments and its environment.
fn agent_command(program: &str, state_dir: &Path, session: Option<&str>) -> Command {
    let mut cmd = Command::new(program);
    // The messages go on stdin, so that no text of the person is read as an option.
    cmd.args(["-p", "--input-format", "stream-json"])
        .args(["--output-format", "stream-json", "--verbose"])
        .args([
            "--replay-user-messages",
            "--append-system-prompt",
            INSTRUCTIONS,
            // No setting source and no MCP server: the personal setup stays out (module text).
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            TOOLS,
        ]);
    if let Some(session) = session {
        cmd.args(["--resume", session]);
    }
    cmd.env("FH_STATE", state_dir)
        .env("PATH", agent_path())
        .env("BASH_DEFAULT_TIMEOUT_MS", BASH_TIMEOUT_MS)
        .env("BASH_MAX_TIMEOUT_MS", BASH_TIMEOUT_MS);
    cmd
}

#[cfg(test)]
mod tests {
    use super::agent_problem;

    #[test]
    fn a_missing_agent_tells_how_to_install_claude_code() {
        let problem = agent_problem("/nonexistent/claude").unwrap();
        assert!(
            problem.contains("curl -fsSL https://claude.ai/install.sh | bash"),
            "{problem}"
        );
    }

    #[test]
    fn an_agent_with_no_login_tells_how_to_sign_in() {
        let problem = agent_problem("false").unwrap();
        assert!(
            problem.contains("sign in with your Claude subscription"),
            "{problem}"
        );
        assert_eq!(agent_problem("true"), None);
    }
}
