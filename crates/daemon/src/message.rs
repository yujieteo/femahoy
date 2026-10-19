//! The text form of the answers of the server and its errors.
//!
//! A message is UTF-8 text. It starts with `key=value` lines, one for each field. Keys are
//! unique. An empty line ends the fields. Item lines can follow it, for example the entries
//! of the chat. A value never holds a line break.

use std::collections::BTreeMap;
use std::fmt;

/// An error of a request, with an HTTP status and a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The HTTP status.
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
        Message::default()
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
