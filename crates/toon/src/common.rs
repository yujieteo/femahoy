//! Common blocks: header, diagnostics, next steps and exit codes (schema section 2).

use crate::wrap::{Cell, Doc, Opts, Table};

pub const VERSION: &str = "1.1.0";
/// Rule R12: a `next` table has at most this many rows.
pub const MAX_NEXT: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warning,
    Error,
    Running,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Error => "error",
            Status::Running => "running",
            Status::Failed => "failed",
        }
    }
}

/// Process exit codes (schema section 2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitCode {
    Success,
    Failure,
    Usage,
    Internal,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        match self {
            ExitCode::Success => 0,
            ExitCode::Failure => 1,
            ExitCode::Usage => 2,
            ExitCode::Internal => 3,
        }
    }
}

/// The header values that differ from one output to the next (rule R2).
#[derive(Clone, Debug)]
pub struct Header {
    pub source: String,
    pub source_hash: String,
    pub units: String,
    pub status: Status,
}

pub(crate) fn start(schema: &str, h: &Header, opts: &Opts) -> Doc {
    Doc::new()
        .scalar("schema", schema)
        .scalar("version", VERSION)
        .scalar("source", h.source.as_str())
        .scalar("source_hash", h.source_hash.as_str())
        .scalar("units", h.units.as_str())
        .scalar("status", h.status.as_str())
        .scalar("precision", opts.precision)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One diagnostic. `code` has a prefix from schema section 2.3.
#[derive(Clone, Debug)]
pub struct Diag {
    pub code: String,
    pub severity: Severity,
    pub line: u32,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Next {
    pub cmd: String,
    pub why: String,
}

pub(crate) fn count(diags: &[Diag], s: Severity) -> usize {
    diags.iter().filter(|d| d.severity == s).count()
}

/// A table from a list of items, in the order given (rules R4, R7 and R8).
pub(crate) fn tbl<const N: usize, R>(
    doc: Doc,
    name: &'static str,
    fields: [&'static str; N],
    note: &'static str,
    rows: impl IntoIterator<Item = R>,
    limit: usize,
    cells: impl Fn(R) -> [Cell; N],
) -> Doc {
    doc.table(Table::new(name, fields, note).ranked(rows.into_iter().map(cells), limit))
}

/// Add the `diagnostics` and `next` tables that end every output (rules R11 and R12).
pub(crate) fn finish(doc: Doc, diags: &[Diag], next: &[Next], opts: &Opts) -> Doc {
    let mut ranked: Vec<&Diag> = diags.iter().collect();
    ranked.sort_by_key(|d| (d.severity != Severity::Error, d.line));
    let doc = tbl(
        doc,
        "diagnostics",
        ["code", "severity", "line", "message"],
        "no diagnostics",
        ranked,
        opts.rows,
        |d| {
            [
                d.code.as_str().into(),
                d.severity.as_str().into(),
                d.line.into(),
                d.message.as_str().into(),
            ]
        },
    );
    // Rule R12 caps `next` at 3 rows, so it never needs a `_more` line.
    let next = next.iter().take(MAX_NEXT);
    tbl(
        doc,
        "next",
        ["cmd", "why"],
        "no next step",
        next,
        MAX_NEXT,
        |n| [n.cmd.as_str().into(), n.why.as_str().into()],
    )
}
