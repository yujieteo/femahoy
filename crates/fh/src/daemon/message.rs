//! The text form of the answers of the server, and the type of its failed requests.
//!
//! A message is UTF-8 text. It starts with `key=value` lines, one for each field. Keys are
//! unique. An empty line ends the fields. Item lines can follow it, for example the entries
//! of the chat. A value never holds a line break. A request is not in this form: the body of a
//! chat message is the plain text of the message.

use std::collections::BTreeMap;
use std::fmt;

/// A failed request: the HTTP status and a plain message.
pub(crate) type Failure = (u16, String);

pub(crate) fn fail(status: u16, message: impl Into<String>) -> Failure {
    (status, message.into())
}

pub(crate) fn bad(message: impl Into<String>) -> Failure {
    fail(400, message)
}

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

    /// Set a field. A control character in the value becomes a space.
    pub fn set(&mut self, key: &str, value: impl fmt::Display) {
        let value = value.to_string().replace(char::is_control, " ");
        self.fields.insert(key.to_owned(), value);
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
