//! The fixture schema: `validation/cases/<id>/case.toml` (see `SCHEMA.md`).
//! Decision D-01, see DESIGN.md.

use std::fmt;
use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, Table};

use crate::ModelSpec;

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

/// The unit system of a case (decision D-12, see DESIGN.md). The case declares it. `fh` never
/// takes a unit from a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Units {
    /// Length mm, force N, stress MPa, mass tonne, time s.
    SiMm,
    /// Length m, force N, stress Pa, mass kg, time s.
    SiM,
}

impl Units {
    /// The text in the case file.
    pub fn name(self) -> &'static str {
        match self {
            Units::SiMm => "SI-mm",
            Units::SiM => "SI-m",
        }
    }

    /// The length unit as gmsh names it (`Geometry.OCCTargetUnit`).
    pub fn gmsh_length(self) -> &'static str {
        match self {
            Units::SiMm => "MM",
            Units::SiM => "M",
        }
    }

    pub fn parse(text: &str) -> Option<Units> {
        match text {
            "SI-mm" => Some(Units::SiMm),
            "SI-m" => Some(Units::SiM),
            _ => None,
        }
    }
}

/// The version of the fixture schema that this reader accepts.
pub const SCHEMA_VERSION: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub dir: PathBuf,
    /// The declared unit system. Every number in the case is in this system.
    pub units: Units,
    pub justification: String,
    pub derivation: PathBuf,
    /// The input file. Absolute or relative to the working directory like `dir`.
    pub mesh: PathBuf,
    /// Pass if `|computed - reference| <= rel_tol * |reference| + abs_tol`.
    pub rel_tol: f64,
    pub abs_tol: f64,
    pub references: Vec<Reference>,
    /// The solver program. `fh-solve-ccx` is the default and the only value now.
    pub solver: String,
    /// The longest time of one solver run, in seconds. `None`: the time limit of the runner.
    pub time_limit_s: Option<f64>,
    /// The most memory of one solver run, in MiB. `None`: the memory limit of the runner.
    pub memory_limit_mib: Option<f64>,
    /// How to build a solver input from a `.msh` file (`fh model`). Optional.
    pub model: Option<ModelSpec>,
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
    /// Load `<dir>/case.toml`. The `mesh` file must exist.
    pub fn load(dir: &Path) -> Result<Case, FixtureError> {
        Case::read(dir, true)
    }

    /// Load `<dir>/case.toml` without the check that the `mesh` file exists. `fh mesh` and
    /// `fh model` use it: the workflow makes the mesh files later.
    pub fn load_before_mesh(dir: &Path) -> Result<Case, FixtureError> {
        Case::read(dir, false)
    }

    fn read(dir: &Path, need_mesh: bool) -> Result<Case, FixtureError> {
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
        let reader = FixtureReader {
            file,
            text: &text,
            need_mesh,
        };
        reader.build(dir, doc.as_table())
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
pub(crate) struct FixtureReader<'a> {
    pub(crate) file: PathBuf,
    pub(crate) text: &'a str,
    /// Refuse a `mesh` file that does not exist.
    pub(crate) need_mesh: bool,
}

impl FixtureReader<'_> {
    pub(crate) fn err(&self, item: Option<&Item>, message: String) -> FixtureError {
        FixtureError {
            file: self.file.clone(),
            line: item
                .and_then(Item::span)
                .map(|s| line_of(self.text, s.start)),
            message,
        }
    }

    pub(crate) fn get<'t>(&self, table: &'t Table, key: &str) -> Result<&'t Item, FixtureError> {
        table
            .get(key)
            .ok_or_else(|| self.err(None, format!("missing key `{key}`")))
    }

    pub(crate) fn string(&self, table: &Table, key: &str) -> Result<String, FixtureError> {
        self.non_empty_string(self.get(table, key)?, key)
            .map(str::to_owned)
    }

    fn non_empty_string<'i>(&self, item: &'i Item, key: &str) -> Result<&'i str, FixtureError> {
        item.as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a non-empty string")))
    }

    pub(crate) fn number(&self, table: &Table, key: &str) -> Result<f64, FixtureError> {
        let item = self.get(table, key)?;
        item.as_float()
            .or_else(|| item.as_integer().map(|i| i as f64))
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a number")))
            .and_then(|value| {
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(self.err(Some(item), format!("`{key}` must be finite")))
                }
            })
    }

    fn tolerance(&self, table: &Table, key: &str) -> Result<f64, FixtureError> {
        let value = self.number(table, key)?;
        if value < 0.0 {
            return Err(self.err(table.get(key), format!("`{key}` must be non-negative")));
        }
        Ok(value)
    }

    /// The `[quantity.<name>]` table of one reference.
    fn quantity(&self, table: &Table, name: &str) -> Result<Quantity, FixtureError> {
        let item = table
            .get("quantity")
            .and_then(Item::as_table_like)
            .and_then(|t| t.get(name))
            .ok_or_else(|| self.err(None, format!("no quantity for reference `{name}`")))?;
        let quantity_table = item
            .as_table_like()
            .ok_or_else(|| self.err(Some(item), format!("`quantity.{name}` must be a table")))?;
        for (k, i) in quantity_table.iter() {
            if !["set", "field", "component", "step", "op"].contains(&k) {
                return Err(self.err(Some(i), format!("unknown key `{k}` in `quantity.{name}`")));
            }
        }
        let get = |key: &str| {
            quantity_table.get(key).ok_or_else(|| {
                self.err(
                    Some(item),
                    format!("missing key `{key}` in `quantity.{name}`"),
                )
            })
        };
        let text = |key: &str| -> Result<(&str, &Item), FixtureError> {
            let item = get(key)?;
            Ok((self.non_empty_string(item, key)?, item))
        };
        let (set, _) = text("set")?;
        let (step, _) = text("step")?;
        let (field_name, field_item) = text("field")?;
        let (field, valid_components): (Field, &[u32]) = match field_name {
            "U" => (Field::U, &[1, 2, 3]),
            "RF" => (Field::RF, &[1, 2, 3]),
            "S" => (Field::S, &[11, 22, 33, 12, 23, 13]),
            other => {
                return Err(self.err(
                    Some(field_item),
                    format!("unknown field `{other}`, expected U, RF or S"),
                ))
            }
        };
        let component_item = get("component")?;
        let component = component_item
            .as_integer()
            .and_then(|c| u32::try_from(c).ok())
            .ok_or_else(|| {
                self.err(
                    Some(component_item),
                    "`component` must be an integer".into(),
                )
            })?;
        if !valid_components.contains(&component) {
            return Err(self.err(
                Some(component_item),
                format!("component {component} is not valid for field {field_name}"),
            ));
        }
        let (op_name, op_item) = text("op")?;
        let op = match op_name {
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
        Ok(Quantity {
            set: set.to_owned(),
            field,
            component,
            step: step.to_owned(),
            op,
        })
    }

    /// The `[ref]` table and the `[quantity.<name>]` table of each entry.
    fn references(&self, table: &Table) -> Result<Vec<Reference>, FixtureError> {
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
            if !value.is_finite() {
                return Err(self.err(Some(item), format!("reference `{name}` must be finite")));
            }
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
        Ok(references)
    }

    fn build(&self, dir: &Path, table: &Table) -> Result<Case, FixtureError> {
        const KEYS: [&str; 14] = [
            "schema",
            "solver",
            "time_limit_s",
            "memory_limit_mib",
            "model",
            "units",
            "quantity",
            "id",
            "justification",
            "derivation",
            "mesh",
            "ref",
            "rel_tol",
            "abs_tol",
        ];
        for (k, item) in table.iter() {
            if !KEYS.contains(&k) {
                return Err(self.err(Some(item), format!("unknown key `{k}`")));
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
        let units_name = self.string(table, "units")?;
        let units = Units::parse(&units_name).ok_or_else(|| {
            self.err(
                table.get("units"),
                format!("unknown units `{units_name}`, expected SI-mm or SI-m"),
            )
        })?;
        let derivation = dir.join(self.string(table, "derivation")?);
        if !derivation.is_file() {
            let item = table.get("derivation");
            return Err(self.err(
                item,
                format!("derivation file {} not found", derivation.display()),
            ));
        }
        let mesh_item = self.get(table, "mesh")?;
        let name = self.non_empty_string(mesh_item, "mesh")?;
        let mesh = dir.join(name);
        if self.need_mesh && !mesh.is_file() {
            return Err(self.err(Some(mesh_item), format!("mesh file {name} not found")));
        }
        let references = self.references(table)?;
        let rel_tol = self.tolerance(table, "rel_tol")?;
        let abs_tol = self.tolerance(table, "abs_tol")?;
        let solver = match table.get("solver") {
            None => "fh-solve-ccx".to_owned(),
            Some(item) => match self.non_empty_string(item, "solver")? {
                s @ "fh-solve-ccx" => s.to_owned(),
                other => {
                    return Err(self.err(
                        Some(item),
                        format!("unknown solver `{other}`, expected fh-solve-ccx"),
                    ))
                }
            },
        };
        let limit = |key: &str| -> Result<Option<f64>, FixtureError> {
            let Some(item) = table.get(key) else {
                return Ok(None);
            };
            match self.number(table, key)? {
                v if v > 0.0 => Ok(Some(v)),
                _ => Err(self.err(Some(item), format!("`{key}` must be above 0"))),
            }
        };
        let (time_limit_s, memory_limit_mib) = (limit("time_limit_s")?, limit("memory_limit_mib")?);
        let model = self.model(table)?;
        Ok(Case {
            id,
            dir: dir.to_owned(),
            units,
            justification,
            derivation,
            mesh,
            rel_tol,
            abs_tol,
            references,
            solver,
            time_limit_s,
            memory_limit_mib,
            model,
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
