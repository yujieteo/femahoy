//! The fixture schema: `validation/cases/<id>/case.toml` (see `SCHEMA.md`).

use std::fmt;
use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, Table};

/// The tolerance class of a case and its parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum Class {
    /// Richardson over the meshes. `ratio` is the mesh size ratio. `reference_error` is relative.
    /// The observed order must lie in `order_min..=order_max`.
    L {
        ratio: f64,
        reference_error: f64,
        order_min: f64,
        order_max: f64,
    },
    /// A fixed tolerance on the one reference mesh.
    N { rel_tol: f64, abs_tol: f64 },
    /// An expected convergence rate and the allowed band.
    R { rate: f64, band: f64 },
}

impl Class {
    pub fn letter(&self) -> char {
        match self {
            Class::L { .. } => 'L',
            Class::N { .. } => 'N',
            Class::R { .. } => 'R',
        }
    }
}

/// The field of a measured quantity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Displacement, a nodal field. Components 1 to 3.
    U,
    /// Reaction force, a nodal field. Components 1 to 3.
    RF,
    /// Stress, an element field. Components 11, 22, 33, 12, 23, 13.
    S,
}

/// How the values of a set reduce to the one number that is compared with the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The set holds one entity. Its value.
    Value,
    Mean,
    Max,
    Min,
    Sum,
}

/// What to measure: a field component, in a node or element set, at the end of a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quantity {
    /// The name of a node set (fields `U`, `RF`) or an element set (field `S`).
    pub set: String,
    pub field: Field,
    pub component: u32,
    /// The name of the step. The result at the end of that step.
    pub step: String,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub name: String,
    pub value: f64,
    pub quantity: Quantity,
}

/// The element types a case may name.
pub const ELEMENT_TYPES: [&str; 4] = ["C3D8", "C3D8I", "C3D10", "C3D10M"];

/// The version of the fixture schema that this reader accepts.
pub const SCHEMA_VERSION: i64 = 2;

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub dir: PathBuf,
    pub class: Class,
    pub justification: String,
    pub derivation: PathBuf,
    /// The element types of the meshes. The case runs once for each type (the meshes hold one).
    pub element_types: Vec<String>,
    /// Input files, coarse to fine. Absolute or relative to the working directory like `dir`.
    pub meshes: Vec<PathBuf>,
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
    /// Load `<dir>/case.toml`.
    pub fn load(dir: &Path) -> Result<Case, FixtureError> {
        let file = dir.join("case.toml");
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
        let fx = Fx { file, text: &text };
        fx.build(dir, doc.as_table())
    }
}

/// The 1-based line of a byte offset.
fn line_of(text: &str, offset: usize) -> usize {
    1 + text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
}

/// The parsed text and its file, for errors with line numbers.
struct Fx<'a> {
    file: PathBuf,
    text: &'a str,
}

impl Fx<'_> {
    fn err(&self, item: Option<&Item>, message: String) -> FixtureError {
        FixtureError {
            file: self.file.clone(),
            line: item
                .and_then(Item::span)
                .map(|s| line_of(self.text, s.start)),
            message,
        }
    }

    fn get<'t>(&self, table: &'t Table, key: &str) -> Result<&'t Item, FixtureError> {
        table
            .get(key)
            .ok_or_else(|| self.err(None, format!("missing key `{key}`")))
    }

    fn string(&self, table: &Table, key: &str) -> Result<String, FixtureError> {
        let item = self.get(table, key)?;
        match item.as_str() {
            Some(s) if !s.is_empty() => Ok(s.to_owned()),
            _ => Err(self.err(Some(item), format!("`{key}` must be a non-empty string"))),
        }
    }

    fn number(&self, table: &Table, key: &str) -> Result<f64, FixtureError> {
        let item = self.get(table, key)?;
        item.as_float()
            .or_else(|| item.as_integer().map(|i| i as f64))
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a number")))
    }

    /// The `[quantity.<name>]` table of one reference.
    fn quantity(&self, table: &Table, name: &str) -> Result<Quantity, FixtureError> {
        let item = table
            .get("quantity")
            .and_then(Item::as_table_like)
            .and_then(|t| t.get(name))
            .ok_or_else(|| self.err(None, format!("no quantity for reference `{name}`")))?;
        let t = item
            .as_table_like()
            .ok_or_else(|| self.err(Some(item), format!("`quantity.{name}` must be a table")))?;
        for (k, i) in t.iter() {
            if !["set", "field", "component", "step", "op"].contains(&k) {
                return Err(self.err(Some(i), format!("unknown key `{k}` in `quantity.{name}`")));
            }
        }
        let get = |key: &str| {
            t.get(key).ok_or_else(|| {
                self.err(
                    Some(item),
                    format!("missing key `{key}` in `quantity.{name}`"),
                )
            })
        };
        let text = |key: &str| -> Result<(String, &Item), FixtureError> {
            let i = get(key)?;
            match i.as_str() {
                Some(s) if !s.is_empty() => Ok((s.to_owned(), i)),
                _ => Err(self.err(Some(i), format!("`{key}` must be a non-empty string"))),
            }
        };
        let (set, _) = text("set")?;
        let (step, _) = text("step")?;
        let (field_name, fi) = text("field")?;
        let (field, valid): (Field, &[u32]) = match field_name.as_str() {
            "U" => (Field::U, &[1, 2, 3]),
            "RF" => (Field::RF, &[1, 2, 3]),
            "S" => (Field::S, &[11, 22, 33, 12, 23, 13]),
            other => {
                return Err(self.err(
                    Some(fi),
                    format!("unknown field `{other}`, expected U, RF or S"),
                ))
            }
        };
        let ci = get("component")?;
        let component = ci
            .as_integer()
            .and_then(|c| u32::try_from(c).ok())
            .ok_or_else(|| self.err(Some(ci), "`component` must be an integer".into()))?;
        if !valid.contains(&component) {
            return Err(self.err(
                Some(ci),
                format!("component {component} is not valid for field {field_name}"),
            ));
        }
        let (word, oi) = text("op")?;
        let op = match word.as_str() {
            "value" => Op::Value,
            "mean" => Op::Mean,
            "max" => Op::Max,
            "min" => Op::Min,
            "sum" => Op::Sum,
            other => {
                return Err(self.err(
                    Some(oi),
                    format!("unknown op `{other}`, expected value, mean, max, min or sum"),
                ))
            }
        };
        Ok(Quantity {
            set,
            field,
            component,
            step,
            op,
        })
    }

    /// An observed-order bound. It must be above 0.
    fn order(&self, table: &Table, key: &str) -> Result<f64, FixtureError> {
        let v = self.number(table, key)?;
        if v > 0.0 {
            Ok(v)
        } else {
            Err(self.err(table.get(key), format!("{key} must be above 0")))
        }
    }

    fn build(&self, dir: &Path, table: &Table) -> Result<Case, FixtureError> {
        const KEYS: [&str; 17] = [
            "schema",
            "quantity",
            "element_types",
            "order_min",
            "order_max",
            "id",
            "class",
            "justification",
            "derivation",
            "mesh",
            "ref",
            "ratio",
            "reference_error",
            "rel_tol",
            "abs_tol",
            "rate",
            "band",
        ];
        for (k, item) in table.iter() {
            if !KEYS.contains(&k) {
                return Err(self.err(Some(item), format!("unknown key `{k}`")));
            }
        }
        // Only the keys of the declared class are allowed.
        let class_name = self.string(table, "class")?;
        let own: &[&str] = match class_name.as_str() {
            "L" => &["ratio", "reference_error", "order_min", "order_max"],
            "N" => &["rel_tol", "abs_tol"],
            "R" => &["rate", "band"],
            _ => &[],
        };
        for (k, item) in table.iter() {
            let is_class_key = [
                "ratio",
                "reference_error",
                "order_min",
                "order_max",
                "rel_tol",
                "abs_tol",
                "rate",
                "band",
            ]
            .contains(&k);
            if is_class_key && !own.contains(&k) && !own.is_empty() {
                return Err(self.err(
                    Some(item),
                    format!("`{k}` does not belong to class {class_name}"),
                ));
            }
        }
        let version = self.get(table, "schema")?;
        if version.as_integer() != Some(SCHEMA_VERSION) {
            return Err(self.err(
                Some(version),
                match version.as_integer() {
                    Some(n) => {
                        format!("schema version {n} is not supported, expected {SCHEMA_VERSION}")
                    }
                    None => "`schema` must be an integer".to_owned(),
                },
            ));
        }
        let id = self.string(table, "id")?;
        let justification = self.string(table, "justification")?;
        let derivation = dir.join(self.string(table, "derivation")?);
        if !derivation.is_file() {
            let item = table.get("derivation");
            return Err(self.err(
                item,
                format!("derivation file {} not found", derivation.display()),
            ));
        }
        let types_item = self.get(table, "element_types")?;
        let mut element_types = Vec::new();
        for v in types_item.as_array().into_iter().flat_map(|a| a.iter()) {
            let t = v.as_str().filter(|t| !t.is_empty()).ok_or_else(|| {
                self.err(
                    Some(types_item),
                    "`element_types` must be an array of strings".into(),
                )
            })?;
            if !ELEMENT_TYPES.contains(&t) {
                return Err(self.err(
                    Some(types_item),
                    format!(
                        "unknown element type `{t}`, expected one of {}",
                        ELEMENT_TYPES.join(", ")
                    ),
                ));
            }
            element_types.push(t.to_owned());
        }
        if element_types.is_empty() {
            return Err(self.err(
                Some(types_item),
                "`element_types` must be a non-empty array of strings".into(),
            ));
        }
        let mesh_item = self.get(table, "mesh")?;
        let list = mesh_item.as_array().ok_or_else(|| {
            self.err(Some(mesh_item), "`mesh` must be an array of strings".into())
        })?;
        let mut meshes = Vec::new();
        for v in list.iter() {
            let name = v.as_str().ok_or_else(|| {
                self.err(Some(mesh_item), "`mesh` must be an array of strings".into())
            })?;
            let path = dir.join(name);
            if !path.is_file() {
                return Err(self.err(Some(mesh_item), format!("mesh file {name} not found")));
            }
            meshes.push(path);
        }
        let ref_item = table
            .get("ref")
            .ok_or_else(|| self.err(None, "missing table `[ref]`".into()))?;
        let ref_table = ref_item
            .as_table_like()
            .ok_or_else(|| self.err(Some(ref_item), "`ref` must be a table".into()))?;
        let mut references = Vec::new();
        for (name, item) in ref_table.iter() {
            let value = item
                .as_float()
                .or_else(|| item.as_integer().map(|i| i as f64))
                .ok_or_else(|| {
                    self.err(Some(item), format!("reference `{name}` must be a number"))
                })?;
            references.push(Reference {
                name: name.to_owned(),
                value,
                quantity: self.quantity(table, name)?,
            });
        }
        if references.is_empty() {
            return Err(self.err(Some(ref_item), "`[ref]` has no entries".into()));
        }
        if let Some(ex) = table.get("quantity").and_then(Item::as_table_like) {
            for (name, item) in ex.iter() {
                if !references.iter().any(|r| r.name == name) {
                    return Err(self.err(
                        Some(item),
                        format!("quantity `{name}` has no reference in `[ref]`"),
                    ));
                }
            }
        }
        let class_item = self.get(table, "class")?;
        let class = match self.string(table, "class")?.as_str() {
            "L" => Class::L {
                ratio: self.number(table, "ratio")?,
                reference_error: self.number(table, "reference_error")?,
                order_min: self.order(table, "order_min")?,
                order_max: self.order(table, "order_max")?,
            },
            "N" => Class::N {
                rel_tol: self.number(table, "rel_tol")?,
                abs_tol: self.number(table, "abs_tol")?,
            },
            "R" => Class::R {
                rate: self.number(table, "rate")?,
                band: self.number(table, "band")?,
            },
            other => {
                return Err(self.err(
                    Some(class_item),
                    format!("unknown class `{other}`, expected L, N or R"),
                ))
            }
        };
        if let Class::L {
            order_min,
            order_max,
            ..
        } = class
        {
            if order_min >= order_max {
                return Err(self.err(
                    table.get("order_min"),
                    "order_min must be below order_max".into(),
                ));
            }
        }
        let (need, ok) = match class {
            Class::L { .. } => ("at least 3", meshes.len() >= 3),
            Class::N { .. } => ("exactly 1", meshes.len() == 1),
            Class::R { .. } => ("at least 2", meshes.len() >= 2),
        };
        if !ok {
            return Err(self.err(
                Some(mesh_item),
                format!(
                    "class {} needs {need} `mesh` entries, found {}",
                    class.letter(),
                    meshes.len()
                ),
            ));
        }
        Ok(Case {
            id,
            dir: dir.to_owned(),
            class,
            justification,
            derivation,
            element_types,
            meshes,
            references,
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
