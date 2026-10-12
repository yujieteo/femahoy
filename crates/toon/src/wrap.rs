//! The only module that calls `toon-format` (rule R1). It builds a document of
//! ordered entries and encodes each entry on its own, so empty tables print as
//! `name[0]:` (rule R8) whatever the encoder does for an empty array.

use std::fmt;
use toon_format::types::{JsonValue, Number};
use toon_format::{encode_object, Delimiter, EncodeOptions};

/// Rule R5: a table has at most this many fields.
pub const MAX_FIELDS: usize = 4;

/// Delimiter between table cells (rule R13). Comma is the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Delim {
    #[default]
    Comma,
    Tab,
}

/// Output options: `--rows`, `--precision` and `--delim`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    pub rows: usize,
    pub precision: usize,
    pub delim: Delim,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            rows: 20,
            precision: 4,
            delim: Delim::Comma,
        }
    }
}

/// The encoder failed. This is a bug in `fh`, so the exit code is 3.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "toon encode failed: {}", self.0)
    }
}

impl std::error::Error for Error {}

/// One table cell or scalar value.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Int(i64),
    Num(f64),
    Text(String),
}

impl From<&str> for Cell {
    fn from(v: &str) -> Self {
        Cell::Text(v.to_string())
    }
}
impl From<String> for Cell {
    fn from(v: String) -> Self {
        Cell::Text(v)
    }
}
impl From<f64> for Cell {
    fn from(v: f64) -> Self {
        Cell::Num(v)
    }
}
impl From<u64> for Cell {
    fn from(v: u64) -> Self {
        Cell::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}
impl From<usize> for Cell {
    fn from(v: usize) -> Self {
        Cell::from(v as u64)
    }
}
impl From<u32> for Cell {
    fn from(v: u32) -> Self {
        Cell::Int(i64::from(v))
    }
}

/// Round to `digits` significant figures (rule R3).
pub fn round_sig(x: f64, digits: usize) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return if x == 0.0 { 0.0 } else { x };
    }
    format!("{:.*e}", digits.max(1) - 1, x).parse().unwrap_or(x)
}

fn json(cell: &Cell, precision: usize) -> JsonValue {
    match cell {
        Cell::Int(i) => JsonValue::Number(Number::from(*i)),
        Cell::Text(t) => JsonValue::String(t.clone()),
        Cell::Num(x) => Number::from_f64(round_sig(*x, precision))
            .map(JsonValue::Number)
            .unwrap_or_else(|| JsonValue::String(x.to_string())),
    }
}

fn object(pairs: Vec<(String, JsonValue)>) -> JsonValue {
    let mut v = JsonValue::Object(Default::default());
    if let Some(map) = v.as_object_mut() {
        map.extend(pairs);
    }
    v
}

/// A table with `N` fields. More than 4 fields fails to compile (rule R5).
#[derive(Clone, Debug)]
pub struct Table<const N: usize> {
    name: &'static str,
    fields: [&'static str; N],
    note: &'static str,
    rows: Vec<[Cell; N]>,
    more: usize,
}

impl<const N: usize> Table<N> {
    /// `note` is the reason that the table prints when it has no rows (rule R8).
    pub fn new(name: &'static str, fields: [&'static str; N], note: &'static str) -> Self {
        const { assert!(N >= 1 && N <= MAX_FIELDS, "a table has 1 to 4 fields") };
        Self {
            name,
            fields,
            note,
            rows: Vec::new(),
            more: 0,
        }
    }

    /// Take rows that are already in rank order. Keep the first `limit` rows
    /// and count the rest (rules R4 and R7).
    pub fn ranked(mut self, rows: impl IntoIterator<Item = [Cell; N]>, limit: usize) -> Self {
        let all: Vec<_> = rows.into_iter().collect();
        self.more = all.len().saturating_sub(limit);
        self.rows = all.into_iter().take(limit).collect();
        self
    }
}

#[derive(Clone, Debug)]
enum Entry {
    Scalar(String, Cell),
    Block(String, Vec<(String, Cell)>),
    Table {
        name: String,
        fields: Vec<String>,
        rows: Vec<Vec<Cell>>,
        more: usize,
        note: String,
    },
}

/// A TOON document: ordered top-level entries.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    entries: Vec<Entry>,
}

impl Doc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn scalar(mut self, key: &str, value: impl Into<Cell>) -> Self {
        self.entries
            .push(Entry::Scalar(key.to_string(), value.into()));
        self
    }

    pub fn block(mut self, key: &str, pairs: Vec<(&str, Cell)>) -> Self {
        let pairs = pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        self.entries.push(Entry::Block(key.to_string(), pairs));
        self
    }

    pub fn table<const N: usize>(mut self, t: Table<N>) -> Self {
        self.entries.push(Entry::Table {
            name: t.name.to_string(),
            fields: t.fields.iter().map(|f| f.to_string()).collect(),
            rows: t.rows.into_iter().map(Vec::from).collect(),
            more: t.more,
            note: t.note.to_string(),
        });
        self
    }

    /// Encode the document with `toon-format`.
    pub fn encode(&self, opts: &Opts) -> Result<String, Error> {
        let delimiter = match opts.delim {
            Delim::Comma => Delimiter::Comma,
            Delim::Tab => Delimiter::Tab,
        };
        let eo = EncodeOptions::new().with_delimiter(delimiter);
        let one = |key: &str, value: JsonValue| {
            encode_object(object(vec![(key.to_string(), value)]), &eo)
                .map_err(|e| Error(e.to_string()))
        };
        let p = opts.precision;
        let mut lines: Vec<String> = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::Scalar(k, v) => lines.push(one(k, json(v, p))?),
                Entry::Block(k, pairs) => {
                    let inner = pairs.iter().map(|(k, v)| (k.clone(), json(v, p))).collect();
                    lines.push(one(k, object(inner))?);
                }
                Entry::Table {
                    name,
                    fields,
                    rows,
                    more,
                    note,
                } => {
                    if rows.is_empty() {
                        lines.push(format!("{name}[0]:"));
                        lines.push(one(
                            &format!("{name}_note"),
                            JsonValue::String(note.clone()),
                        )?);
                        continue;
                    }
                    let objs = rows
                        .iter()
                        .map(|r| {
                            let pairs = fields.iter().cloned().zip(r.iter().map(|c| json(c, p)));
                            object(pairs.collect())
                        })
                        .collect();
                    lines.push(one(name, JsonValue::Array(objs))?);
                    if *more > 0 {
                        let n = JsonValue::Number(Number::from(*more as u64));
                        lines.push(one(&format!("{name}_more"), n)?);
                    }
                }
            }
        }
        lines.push(String::new());
        Ok(lines.join("\n"))
    }
}
