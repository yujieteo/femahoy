use std::fmt;

/// Stable diagnostic codes of the `.inp` reader. `docs/diagnostics.md` lists them.
pub mod code {
    pub const SYNTAX: &str = "INP001";
    pub const UNSUPPORTED_KEYWORD: &str = "INP002";
    pub const UNSUPPORTED_PARAMETER: &str = "INP003";
    pub const MISSING_PARAMETER: &str = "INP004";
    pub const UNSUPPORTED_VALUE: &str = "INP005";
    pub const INVALID_VALUE: &str = "INP006";
    pub const BAD_DATA_LINE: &str = "INP007";
    pub const DUPLICATE: &str = "INP008";
    pub const UNDEFINED: &str = "INP009";
    pub const OUT_OF_PLACE: &str = "INP010";
    pub const INCOMPLETE: &str = "INP011";
}

/// A parse error. It has a stable code, the line number and, when known, the
/// keyword and the file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub file: Option<String>,
    pub line: usize,
    pub keyword: Option<String>,
    pub message: String,
}

impl ParseError {
    pub(crate) fn new(
        code: &'static str,
        line: usize,
        keyword: Option<&str>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            file: None,
            line,
            keyword: keyword.map(str::to_owned),
            message: message.into(),
        }
    }

    /// The same error with the name of the file that holds the line.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}: ")?;
        }
        match &self.keyword {
            Some(k) => write!(f, "line {}, *{}: {}", self.line, k, self.message)?,
            None => write!(f, "line {}: {}", self.line, self.message)?,
        }
        write!(f, " ({})", self.code)
    }
}

impl std::error::Error for ParseError {}
