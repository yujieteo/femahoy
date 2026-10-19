//! The run contract, version 2: run IDs, run states, run records, events and the text form
//! that the daemon and its clients share.
//!
//! # Text form
//!
//! A message is UTF-8 text. It starts with `key=value` lines, one for each field. Keys are
//! unique. An empty line ends the fields. Item lines can follow it, for example the events
//! of a run. A value never holds a line break.

use std::collections::BTreeMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// The version of the run contract. Each message has the field `contract`.
pub const CONTRACT: &str = "2";

/// An error of a command, with an HTTP status and a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The HTTP status. A client that cannot reach the daemon gets 0.
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl Error {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Error {
            status,
            code: code.to_owned(),
            message: message.into(),
        }
    }

    pub(crate) fn io(what: &str, e: std::io::Error) -> Self {
        Error::new(500, "store.io", format!("{what}: {e}"))
    }

    pub(crate) fn bad(code: &str, message: impl Into<String>) -> Self {
        Error::new(400, code, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// A message in the text form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message {
    pub fields: BTreeMap<String, String>,
    pub items: Vec<String>,
}

impl Message {
    pub fn new() -> Self {
        let mut m = Message::default();
        m.set("contract", CONTRACT);
        m
    }

    pub fn parse(text: &str) -> Result<Message, Error> {
        let mut m = Message::default();
        let mut lines = text.lines();
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(Error::bad("message.invalid", format!("no '=' in {line:?}")));
            };
            if m.fields.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(Error::bad("message.invalid", format!("two fields {key}")));
            }
        }
        m.items = lines.map(str::to_owned).collect();
        Ok(m)
    }

    /// Set a field. A control character in the value becomes a space.
    pub fn set(&mut self, key: &str, value: impl fmt::Display) {
        let value = value.to_string().replace(char::is_control, " ");
        self.fields.insert(key.to_owned(), value);
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }

    /// A field that the message must have.
    pub fn need(&self, key: &str) -> Result<&str, Error> {
        self.get(key)
            .ok_or_else(|| Error::bad("field.missing", format!("no field {key}")))
    }

    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for (k, v) in &self.fields {
            text.push_str(&format!("{k}={v}\n"));
        }
        if !self.items.is_empty() {
            text.push('\n');
            for item in &self.items {
                text.push_str(&item.replace(['\n', '\r'], " "));
                text.push('\n');
            }
        }
        text
    }
}

/// A run ID: `run-` and 6 or more digits. Run IDs count up in each state folder.
pub fn valid_run(id: &str) -> bool {
    id.strip_prefix("run-")
        .is_some_and(|n| n.len() >= 6 && n.bytes().all(|b| b.is_ascii_digit()))
}

pub(crate) fn run_id(number: u64) -> String {
    format!("run-{number:06}")
}

/// The state of a run.
///
/// `queued -> running -> succeeded | failed | cancelling | interrupted`,
/// `queued -> cancelled | interrupted` and `cancelling -> cancelled | succeeded | failed`.
/// A final state never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    /// The daemon stopped before the run ended. The daemon never starts the run again.
    Interrupted,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Queued => "queued",
            RunState::Running => "running",
            RunState::Cancelling => "cancelling",
            RunState::Succeeded => "succeeded",
            RunState::Failed => "failed",
            RunState::Cancelled => "cancelled",
            RunState::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<RunState> {
        [
            RunState::Queued,
            RunState::Running,
            RunState::Cancelling,
            RunState::Succeeded,
            RunState::Failed,
            RunState::Cancelled,
            RunState::Interrupted,
        ]
        .into_iter()
        .find(|state| state.as_str() == s)
    }

    pub fn is_final(self) -> bool {
        !matches!(
            self,
            RunState::Queued | RunState::Running | RunState::Cancelling
        )
    }

    pub fn can_move_to(self, next: RunState) -> bool {
        use RunState::*;
        matches!(
            (self, next),
            (Queued, Running | Cancelled | Interrupted)
                | (Running, Cancelling | Succeeded | Failed | Interrupted)
                | (Cancelling, Cancelled | Succeeded | Failed)
        )
    }
}

impl fmt::Display for RunState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

fn parse_state(s: &str) -> Result<RunState, Error> {
    RunState::parse(s).ok_or_else(|| Error::bad("state.invalid", format!("no run state {s}")))
}

/// The one record of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    pub run: String,
    /// The path of the input file that the client gave. The run solves a copy of it.
    pub input: String,
    pub state: RunState,
    /// Times in milliseconds since the Unix epoch. 0 means not yet.
    pub created_ms: u64,
    pub started_ms: u64,
    pub finished_ms: u64,
    /// Why the run failed, or was cancelled or interrupted.
    pub message: String,
}

impl RunRecord {
    pub fn to_message(&self) -> Message {
        let mut m = Message::new();
        m.set("run", &self.run);
        m.set("input", &self.input);
        m.set("state", self.state);
        m.set("created_ms", self.created_ms);
        m.set("started_ms", self.started_ms);
        m.set("finished_ms", self.finished_ms);
        m.set("message", &self.message);
        m
    }

    pub fn from_message(m: &Message) -> Result<RunRecord, Error> {
        let num = |key: &str| {
            m.need(key)?
                .parse()
                .map_err(|_| Error::bad("field.invalid", format!("{key} is not a number")))
        };
        Ok(RunRecord {
            run: m.need("run")?.to_owned(),
            input: m.need("input")?.to_owned(),
            state: parse_state(m.need("state")?)?,
            created_ms: num("created_ms")?,
            started_ms: num("started_ms")?,
            finished_ms: num("finished_ms")?,
            message: m.get("message").unwrap_or_default().to_owned(),
        })
    }
}

/// One event of a run. Sequence numbers start at 1 and count up in each run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub seq: u64,
    pub text: String,
}

/// The events of a run after a sequence number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Events {
    /// The events, in order.
    pub items: Vec<Event>,
    /// The last sequence number of the run so far. 0 when there is no event.
    pub last: u64,
    /// The number of events after `since` that the bounded queue no longer has.
    pub dropped: u64,
    /// The state of the run.
    pub state: RunState,
}

impl Events {
    pub fn to_message(&self) -> Message {
        let mut m = Message::new();
        m.set("last", self.last);
        m.set("dropped", self.dropped);
        m.set("state", self.state);
        m.items = self
            .items
            .iter()
            .map(|e| format!("{} {}", e.seq, e.text))
            .collect();
        m
    }

    pub fn from_message(m: &Message) -> Result<Events, Error> {
        let num = |key: &str| {
            m.need(key)?
                .parse()
                .map_err(|_| Error::bad("field.invalid", format!("{key} is not a number")))
        };
        let items = m
            .items
            .iter()
            .map(|line| parse_event(line))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::bad("message.invalid", "bad event line"))?;
        Ok(Events {
            items,
            last: num("last")?,
            dropped: num("dropped")?,
            state: parse_state(m.need("state")?)?,
        })
    }
}

/// An event line: the sequence number, a space and the text.
pub(crate) fn parse_event(line: &str) -> Option<Event> {
    let (seq, text) = line.split_once(' ')?;
    Some(Event {
        seq: seq.parse().ok()?,
        text: text.to_owned(),
    })
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_fields_and_items() {
        let mut m = Message::new();
        m.set("message", "two\nlines");
        m.items = vec!["1 state queued".into()];
        let text = m.to_text();
        assert_eq!(text, "contract=2\nmessage=two lines\n\n1 state queued\n");
        assert_eq!(Message::parse(&text).unwrap(), m);
        assert!(Message::parse("a=1\na=2\n").is_err());
        assert!(Message::parse("no field\n").is_err());
    }

    #[test]
    fn final_states_never_change() {
        let all = [
            RunState::Queued,
            RunState::Running,
            RunState::Cancelling,
            RunState::Succeeded,
            RunState::Failed,
            RunState::Cancelled,
            RunState::Interrupted,
        ];
        for from in all {
            assert_eq!(RunState::parse(from.as_str()), Some(from));
            for to in all {
                if from.is_final() {
                    assert!(!from.can_move_to(to), "{from} -> {to}");
                }
            }
        }
        assert!(RunState::Cancelling.can_move_to(RunState::Succeeded));
        assert!(!RunState::Cancelling.can_move_to(RunState::Running));
    }

    #[test]
    fn run_ids_are_checked() {
        assert!(valid_run(&run_id(7)));
        assert_eq!(run_id(7), "run-000007");
        assert!(!valid_run("run-7"));
        assert!(!valid_run("../x"));
    }
}
