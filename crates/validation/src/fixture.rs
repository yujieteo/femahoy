//! The check file of a case: `validation/cases/<id>/check.toml` (see `docs/reference/check-toml.md`).

use std::fmt;
use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, TableLike};

/// The field of a measured quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Displacement, from `*NODE PRINT`. Components 1 to 3.
    U,
    /// Reaction force, from `*NODE PRINT`. Components 1 to 3.
    RF,
    /// Stress at each integration point, from `*EL PRINT`. Components 11, 22, 33, 12, 13, 23.
    S,
    /// Temperature, from `*NODE PRINT`. Component 1.
    NT,
    /// Contact stress at each point of all contact elements, from `*CONTACT PRINT`. Component 1
    /// is the pressure, 2 and 3 are the shear stresses. It has no set.
    Cstr,
    /// The frequency of a mode (cycles per time unit), from a `*FREQUENCY` step.
    Freq,
    /// The buckling factor of a mode, from a `*BUCKLE` step.
    Buckle,
}

/// How the values of a set reduce to the one number that is compared with the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The set holds one value. That value.
    Value,
    Mean,
    Max,
    Min,
    Sum,
}

/// What to measure: a field component, in a node or element set, at the end of the last step.
/// `Freq` and `Buckle` have no set: `component` is the mode number and `op` is `Value`.
/// `Cstr` has no set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quantity {
    /// The name of the set that the print request of the input names.
    pub set: String,
    pub field: Field,
    pub component: u32,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub name: String,
    pub value: f64,
    pub quantity: Quantity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The name of the folder.
    pub id: String,
    pub dir: PathBuf,
    /// The CalculiX input, `<dir>/job.inp`.
    pub input: PathBuf,
    /// Pass if `|computed - reference| <= rel_tol * |reference| + abs_tol`.
    pub rel_tol: f64,
    pub abs_tol: f64,
    pub references: Vec<Reference>,
}

/// A fixture error. It names the file and, when known, the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureError {
    pub file: PathBuf,
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => write!(f, "{}:{l}: {}", self.file.display(), self.message),
            None => write!(f, "{}: {}", self.file.display(), self.message),
        }
    }
}

impl std::error::Error for FixtureError {}

impl Case {
    /// Load `<dir>/check.toml`. The file `<dir>/job.inp` must exist.
    pub fn load(dir: &Path) -> Result<Case, FixtureError> {
        let file = dir.join("check.toml");
        let text = std::fs::read_to_string(&file).map_err(|e| FixtureError {
            file: file.clone(),
            line: None,
            message: e.to_string(),
        })?;
        let doc = Document::parse(text.as_str()).map_err(|e| FixtureError {
            file: file.clone(),
            line: e.span().map(|s| line_of(&text, s.start)),
            message: e.message().to_owned(),
        })?;
        let reader = Reader { file, text: &text };
        let input = dir.join("job.inp");
        if !input.is_file() {
            return Err(reader.err(None, format!("{} not found", input.display())));
        }
        let table: &dyn TableLike = doc.as_table();
        let mut references = Vec::new();
        for (name, item) in table.iter() {
            if name != "rel_tol" && name != "abs_tol" {
                references.push(reader.reference(name, item)?);
            }
        }
        if references.is_empty() {
            return Err(reader.err(None, "no check: add one `[name]` table".into()));
        }
        Ok(Case {
            id: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir: dir.to_owned(),
            input,
            rel_tol: reader.tolerance(table, "rel_tol")?,
            abs_tol: reader.tolerance(table, "abs_tol")?,
            references,
        })
    }
}

/// The 1-based line of a byte offset.
fn line_of(text: &str, offset: usize) -> usize {
    1 + text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

/// The text and its file, for errors with line numbers.
struct Reader<'a> {
    file: PathBuf,
    text: &'a str,
}

impl Reader<'_> {
    fn err(&self, item: Option<&Item>, message: String) -> FixtureError {
        FixtureError {
            file: self.file.clone(),
            line: item
                .and_then(Item::span)
                .map(|s| line_of(self.text, s.start)),
            message,
        }
    }

    fn get<'t>(
        &self,
        table: &'t dyn TableLike,
        key: &str,
        within: Option<&Item>,
    ) -> Result<&'t Item, FixtureError> {
        table
            .get(key)
            .ok_or_else(|| self.err(within, format!("missing key `{key}`")))
    }

    fn string<'t>(&self, item: &'t Item, key: &str) -> Result<&'t str, FixtureError> {
        item.as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a non-empty string")))
    }

    fn finite(&self, item: &Item, key: &str) -> Result<f64, FixtureError> {
        let value = item
            .as_float()
            .or_else(|| item.as_integer().map(|i| i as f64))
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a number")))?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(self.err(Some(item), format!("`{key}` must be finite")))
        }
    }

    fn tolerance(&self, table: &dyn TableLike, key: &str) -> Result<f64, FixtureError> {
        let item = self.get(table, key, None)?;
        let value = self.finite(item, key)?;
        if value < 0.0 {
            return Err(self.err(Some(item), format!("`{key}` must be non-negative")));
        }
        Ok(value)
    }

    /// One check: the `[name]` table with `value`, `set`, `field`, `component` and `op`, or
    /// with `value`, `field` and `mode` for `FREQ` and `BUCKLE`.
    fn reference(&self, name: &str, item: &Item) -> Result<Reference, FixtureError> {
        let table = item
            .as_table_like()
            .ok_or_else(|| self.err(Some(item), format!("`{name}` must be a table")))?;
        let get = |key: &str| self.get(table, key, Some(item));
        let value = self.finite(get("value")?, "value")?;
        let field_item = get("field")?;
        let (field, components): (Field, &[u32]) = match self.string(field_item, "field")? {
            "U" => (Field::U, &[1, 2, 3]),
            "RF" => (Field::RF, &[1, 2, 3]),
            "S" => (Field::S, &[11, 22, 33, 12, 13, 23]),
            "NT" => (Field::NT, &[1]),
            "CSTR" => (Field::Cstr, &[1, 2, 3]),
            "FREQ" => (Field::Freq, &[]),
            "BUCKLE" => (Field::Buckle, &[]),
            other => {
                return Err(self.err(
                    Some(field_item),
                    format!("unknown field `{other}`, expected U, RF, S, NT, CSTR, FREQ or BUCKLE"),
                ))
            }
        };
        let keys: &[&str] = match field {
            Field::Freq | Field::Buckle => &["value", "field", "mode"],
            Field::Cstr => &["value", "field", "component", "op"],
            _ => &["value", "set", "field", "component", "op"],
        };
        for (key, value) in table.iter() {
            if !keys.contains(&key) {
                return Err(self.err(Some(value), format!("unknown key `{key}` in `{name}`")));
            }
        }
        if components.is_empty() {
            let mode_item = get("mode")?;
            let mode = mode_item
                .as_integer()
                .and_then(|m| u32::try_from(m).ok())
                .filter(|m| *m >= 1)
                .ok_or_else(|| self.err(Some(mode_item), "`mode` must be 1 or more".into()))?;
            return Ok(Reference {
                name: name.to_owned(),
                value,
                quantity: Quantity {
                    set: String::new(),
                    field,
                    component: mode,
                    op: Op::Value,
                },
            });
        }
        let set = match field {
            Field::Cstr => String::new(),
            _ => self.string(get("set")?, "set")?.to_owned(),
        };
        let component_item = get("component")?;
        let component = component_item
            .as_integer()
            .and_then(|c| u32::try_from(c).ok())
            .filter(|c| components.contains(c))
            .ok_or_else(|| {
                self.err(
                    Some(component_item),
                    format!("`component` must be one of {components:?} for this field"),
                )
            })?;
        let op_item = get("op")?;
        let op = match self.string(op_item, "op")? {
            "value" => Op::Value,
            "mean" => Op::Mean,
            "max" => Op::Max,
            "min" => Op::Min,
            "sum" => Op::Sum,
            other => {
                return Err(self.err(
                    Some(op_item),
                    format!("unknown op `{other}`, expected value, mean, max, min or sum"),
                ))
            }
        };
        Ok(Reference {
            name: name.to_owned(),
            value,
            quantity: Quantity {
                set,
                field,
                component,
                op,
            },
        })
    }
}

/// Load every case folder under `root`, sorted by folder name.
pub fn load_all(root: &Path) -> Result<Vec<Case>, FixtureError> {
    let io = |e: std::io::Error| FixtureError {
        file: root.to_owned(),
        line: None,
        message: e.to_string(),
    };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(root).map_err(io)? {
        let path = entry.map_err(io)?.path();
        if path.is_dir() {
            dirs.push(path);
        }
    }
    dirs.sort();
    dirs.iter().map(|d| Case::load(d)).collect()
}
