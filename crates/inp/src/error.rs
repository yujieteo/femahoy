use std::fmt;

/// A parse error. It names the line number and, when known, the keyword.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub keyword: Option<String>,
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(line: usize, keyword: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            line,
            keyword: keyword.map(str::to_owned),
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.keyword {
            Some(k) => write!(f, "line {}, *{}: {}", self.line, k, self.message),
            None => write!(f, "line {}: {}", self.line, self.message),
        }
    }
}

impl std::error::Error for ParseError {}
