//! The fixture schema: `validation/cases/<id>/case.kv` (see `SCHEMA.md`).

use std::fmt;
use std::path::{Path, PathBuf};

/// The tolerance class of a case and its parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum Class {
    /// Richardson over the meshes. `ratio` is the mesh size ratio. `reference_error` is relative.
    L { ratio: f64, reference_error: f64 },
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

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub id: String,
    pub dir: PathBuf,
    pub class: Class,
    pub justification: String,
    pub derivation: PathBuf,
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
    /// Load `<dir>/case.kv`.
    pub fn load(dir: &Path) -> Result<Case, FixtureError> {
        let file = dir.join("case.kv");
        let err = |line: Option<usize>, message: String| FixtureError {
            file: file.clone(),
            line,
            message,
        };
        let text = std::fs::read_to_string(&file).map_err(|e| err(None, e.to_string()))?;
        let mut kv: Vec<(usize, String, String)> = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(err(
                    Some(i + 1),
                    format!("expected `key = value`, found `{line}` (no `=`)"),
                ));
            };
            kv.push((i + 1, k.trim().to_owned(), v.trim().to_owned()));
        }
        const KEYS: [&str; 12] = [
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
        for (line, k, _) in &kv {
            if !KEYS.contains(&k.as_str()) {
                return Err(err(Some(*line), format!("unknown key `{k}`")));
            }
        }
        let all = |key: &str| kv.iter().filter(|(_, k, _)| k == key).collect::<Vec<_>>();
        let one = |key: &str| -> Result<(usize, String), FixtureError> {
            let mut it = all(key).into_iter();
            let first = it
                .next()
                .ok_or_else(|| err(None, format!("missing key `{key}`")))?;
            match it.next() {
                Some((l, _, _)) => Err(err(Some(*l), format!("key `{key}` given twice"))),
                None => Ok((first.0, first.2.clone())),
            }
        };
        let num = |key: &str| -> Result<f64, FixtureError> {
            let (l, v) = one(key)?;
            v.parse::<f64>()
                .map_err(|_| err(Some(l), format!("`{key}` is not a number: `{v}`")))
        };
        let text_of = |key: &str| -> Result<String, FixtureError> {
            let (l, v) = one(key)?;
            if v.is_empty() {
                return Err(err(Some(l), format!("`{key}` is empty")));
            }
            Ok(v)
        };

        let id = text_of("id")?;
        let justification = text_of("justification")?;
        let derivation = dir.join(text_of("derivation")?);
        if !derivation.is_file() {
            return Err(err(
                None,
                format!("derivation file {} not found", derivation.display()),
            ));
        }
        let mut meshes = Vec::new();
        for (l, _, v) in all("mesh") {
            let p = dir.join(v);
            if !p.is_file() {
                return Err(err(Some(*l), format!("mesh file {v} not found")));
            }
            meshes.push(p);
        }
        let mut references = Vec::new();
        for (l, _, v) in all("ref") {
            let mut parts = v.split_whitespace();
            match (
                parts.next(),
                parts.next().map(str::parse::<f64>),
                parts.next(),
            ) {
                (Some(name), Some(Ok(value)), None) => references.push(Reference {
                    name: name.to_owned(),
                    value,
                }),
                _ => {
                    return Err(err(
                        Some(*l),
                        format!("expected `ref = <name> <value>`, found `{v}`"),
                    ))
                }
            }
        }
        if references.is_empty() {
            return Err(err(None, "missing key `ref`".into()));
        }
        let (class_line, class_text) = one("class")?;
        let class = match class_text.as_str() {
            "L" => Class::L {
                ratio: num("ratio")?,
                reference_error: num("reference_error")?,
            },
            "N" => Class::N {
                rel_tol: num("rel_tol")?,
                abs_tol: num("abs_tol")?,
            },
            "R" => Class::R {
                rate: num("rate")?,
                band: num("band")?,
            },
            other => {
                return Err(err(
                    Some(class_line),
                    format!("unknown class `{other}`, expected L, N or R"),
                ))
            }
        };
        let count_ok = match class {
            Class::L { .. } => meshes.len() >= 3,
            Class::N { .. } => meshes.len() == 1,
            Class::R { .. } => meshes.len() >= 2,
        };
        if !count_ok {
            let need = match class {
                Class::L { .. } => "at least 3",
                Class::N { .. } => "exactly 1",
                Class::R { .. } => "at least 2",
            };
            return Err(err(
                None,
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
