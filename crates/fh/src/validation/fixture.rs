//! The check file of a case: `validation/cases/<id>/check.toml` (see `docs/reference/check-toml.md`).
//!
//! A case is a CalculiX case (`job.inp`) or a set of OpenFOAM, GetDP, preCICE, Chrono, LAMMPS,
//! Meep, topology, shape optimization or fit cases in subfolders.

use std::path::{Path, PathBuf};
use toml_edit::{Document, Item, TableLike};

/// The field of a measured quantity.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// The RMS of the acceleration (2 pi f)^2 U at each node over the frequencies of a `*STEADY
    /// STATE DYNAMICS` step, from `*NODE PRINT` (see `crate::ccx::rms`). Components 1 to 3.
    ARms,
    /// A column of a text table that an OpenFOAM, GetDP, LAMMPS or topology case writes, for example the
    /// column `Cl` of `postProcessing/forceCoeffs/0/coefficient.dat` of a function object of
    /// OpenFOAM.
    Table {
        /// The path of the table in the folder of the case.
        file: String,
        /// The name of the column in the last comment line of the table.
        column: String,
    },
}

/// How the values of a set reduce to the one number that is compared with the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The set holds one value. That value.
    Value,
    Mean,
    Max,
    Sum,
}

/// What to measure: a field component, in a node or element set, at the end of the last step.
/// `Freq` and `Buckle` have no set: `component` is the mode number and `op` is `Value`.
/// `Cstr` has no set. `Table` is the value in the last row of the table, for example the value of
/// the last iteration of an OpenFOAM case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quantity {
    /// The name of the set that the print request of the input names. For `Table`, the subfolder
    /// of the OpenFOAM, GetDP, LAMMPS or topology case.
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

impl Reference {
    /// True when the check reads a table of an OpenFOAM, GetDP, LAMMPS or topology case.
    fn table(&self) -> bool {
        matches!(self.quantity.field, Field::Table { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The name of the folder.
    pub id: String,
    pub dir: PathBuf,
    /// The CalculiX input, `<dir>/job.inp`, or `<dir>` when the checks are tables.
    pub input: PathBuf,
    /// Pass if `|computed - reference| <= rel_tol * |reference| + abs_tol`.
    pub rel_tol: f64,
    pub abs_tol: f64,
    pub references: Vec<Reference>,
}

/// An error text of a fixture. It names the file and, when known, the line.
fn located(file: &Path, line: Option<usize>, message: &str) -> String {
    match line {
        Some(l) => format!("{}:{l}: {message}", file.display()),
        None => format!("{}: {message}", file.display()),
    }
}

impl Case {
    /// True when the checks are tables of the OpenFOAM, GetDP, preCICE, LAMMPS or topology cases
    /// in the subfolders.
    pub fn tables(&self) -> bool {
        self.references.iter().any(Reference::table)
    }

    /// Load `<dir>/check.toml`. The file `<dir>/job.inp` must exist, or, when the checks are
    /// tables, the OpenFOAM, GetDP, preCICE, LAMMPS or topology case of each check.
    pub fn load(dir: &Path) -> Result<Case, String> {
        let file = dir.join("check.toml");
        let text =
            std::fs::read_to_string(&file).map_err(|e| located(&file, None, &e.to_string()))?;
        let doc = Document::parse(text.as_str()).map_err(|e| {
            let line = e.span().map(|s| line_of(&text, s.start));
            located(&file, line, e.message())
        })?;
        let reader = Reader { file, text: &text };
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
        let input = if references.iter().all(Reference::table) {
            for r in &references {
                let case = dir.join(&r.quantity.set);
                let solvers = [
                    crate::foam::is_case,
                    crate::getdp::is_case,
                    crate::chrono::is_case,
                    crate::meep::is_case,
                    crate::precice::is_case,
                    crate::lammps::is_case,
                    crate::topology::is_case,
                    crate::optimize::is_case,
                    crate::fit::is_case,
                ];
                if !solvers.iter().any(|is_case| is_case(&case)) {
                    let (pro, dict) = (case.join("case.pro"), case.join("system/controlDict"));
                    let missing = format!(
                        "{}, {}, {}, {}, {}, {}, {}, {} or {} not found",
                        pro.display(),
                        case.join("chrono.py").display(),
                        case.join("meep_case.py").display(),
                        case.join("topology.toml").display(),
                        case.join("optimize.toml").display(),
                        case.join("fit.toml").display(),
                        dict.display(),
                        case.join("precice-config.xml").display(),
                        case.join("case.lmp").display()
                    );
                    return Err(reader.err(None, missing));
                }
            }
            dir.to_owned()
        } else if references.iter().any(Reference::table) {
            return Err(reader.err(
                None,
                "a case has table checks (with `case`) only, or no table check".into(),
            ));
        } else {
            dir.join("job.inp")
        };
        if !input.exists() {
            return Err(reader.err(None, format!("{} not found", input.display())));
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

/// True when `name` is the name of a file or a folder: letters, digits, `.`, `_` and `-`, and not
/// `.` first. Thus it is not `..`.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
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
    fn err(&self, item: Option<&Item>, message: String) -> String {
        let line = item
            .and_then(Item::span)
            .map(|s| line_of(self.text, s.start));
        located(&self.file, line, &message)
    }

    fn get<'t>(
        &self,
        table: &'t dyn TableLike,
        key: &str,
        within: Option<&Item>,
    ) -> Result<&'t Item, String> {
        table
            .get(key)
            .ok_or_else(|| self.err(within, format!("missing key `{key}`")))
    }

    fn string<'t>(&self, item: &'t Item, key: &str) -> Result<&'t str, String> {
        item.as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be a non-empty string")))
    }

    fn finite(&self, item: &Item, key: &str) -> Result<f64, String> {
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

    fn tolerance(&self, table: &dyn TableLike, key: &str) -> Result<f64, String> {
        let item = self.get(table, key, None)?;
        let value = self.finite(item, key)?;
        if value < 0.0 {
            return Err(self.err(Some(item), format!("`{key}` must be non-negative")));
        }
        Ok(value)
    }

    /// One check: the `[name]` table with `value`, `set`, `field`, `component` and `op`, or
    /// with `value`, `field` and `mode` for `FREQ` and `BUCKLE`, or with `value`, `case`, `file`
    /// and `column` for a table of an OpenFOAM, GetDP, LAMMPS or topology case.
    fn reference(&self, name: &str, item: &Item) -> Result<Reference, String> {
        let table = item
            .as_table_like()
            .ok_or_else(|| self.err(Some(item), format!("`{name}` must be a table")))?;
        let get = |key: &str| self.get(table, key, Some(item));
        let only = |keys: &[&str]| {
            for (key, value) in table.iter() {
                if !keys.contains(&key) {
                    return Err(self.err(Some(value), format!("unknown key `{key}` in `{name}`")));
                }
            }
            Ok(())
        };
        let value = self.finite(get("value")?, "value")?;
        if table.contains_key("case") {
            only(&["value", "case", "file", "column"])?;
            let case_item = get("case")?;
            let case = self.string(case_item, "case")?;
            if !plain(case) {
                return Err(self.err(
                    Some(case_item),
                    "`case` must be the name of a subfolder: letters, digits, `.`, `_` and `-`"
                        .into(),
                ));
            }
            let file_item = get("file")?;
            let file = self.string(file_item, "file")?;
            if !file.split('/').all(plain) {
                return Err(self.err(
                    Some(file_item),
                    "`file` must be a path in the case folder: names of letters, digits, `.`, `_` \
                     and `-`, with `/` between them"
                        .into(),
                ));
            }
            let column = self.string(get("column")?, "column")?;
            return Ok(Reference {
                name: name.to_owned(),
                value,
                quantity: Quantity {
                    set: case.to_owned(),
                    field: Field::Table {
                        file: file.to_owned(),
                        column: column.to_owned(),
                    },
                    component: 0,
                    op: Op::Value,
                },
            });
        }
        let field_item = get("field")?;
        let (field, components): (Field, &[u32]) = match self.string(field_item, "field")? {
            "U" => (Field::U, &[1, 2, 3]),
            "RF" => (Field::RF, &[1, 2, 3]),
            "S" => (Field::S, &[11, 22, 33, 12, 13, 23]),
            "NT" => (Field::NT, &[1]),
            "CSTR" => (Field::Cstr, &[1, 2, 3]),
            "FREQ" => (Field::Freq, &[]),
            "BUCKLE" => (Field::Buckle, &[]),
            "A_RMS" => (Field::ARms, &[1, 2, 3]),
            other => {
                return Err(self.err(
                    Some(field_item),
                    format!(
                        "unknown field `{other}`, expected U, RF, S, NT, CSTR, FREQ, BUCKLE \
                         or A_RMS"
                    ),
                ))
            }
        };
        only(match field {
            Field::Freq | Field::Buckle => &["value", "field", "mode"],
            Field::Cstr => &["value", "field", "component", "op"],
            _ => &["value", "set", "field", "component", "op"],
        })?;
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
            "sum" => Op::Sum,
            other => {
                return Err(self.err(
                    Some(op_item),
                    format!("unknown op `{other}`, expected value, mean, max or sum"),
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
pub fn load_all(root: &Path) -> Result<Vec<Case>, String> {
    let io = |e: std::io::Error| located(root, None, &e.to_string());
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
