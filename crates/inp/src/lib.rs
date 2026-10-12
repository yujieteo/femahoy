//! `.inp` reader and writer, Abaqus-shaped model, template mapping.
//!
//! M1 scope: the reader for the linear static keyword subset. The writer and
//! the template mapping come later.

mod parse;

use std::collections::BTreeMap;
use std::fmt;

pub use parse::parse;

/// A parsed model. Names of sets and materials are upper case.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub nsets: BTreeMap<String, Vec<u32>>,
    pub elsets: BTreeMap<String, Vec<u32>>,
    pub materials: Vec<Material>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub id: u32,
    pub coords: [f64; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    C3D8I,
    C3D8,
    C3D10,
    C3D10M,
}

impl ElementKind {
    /// Parses an element type name, such as `C3D8I`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "C3D8I" => Some(Self::C3D8I),
            "C3D8" => Some(Self::C3D8),
            "C3D10" => Some(Self::C3D10),
            "C3D10M" => Some(Self::C3D10M),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::C3D8I => "C3D8I",
            Self::C3D8 => "C3D8",
            Self::C3D10 => "C3D10",
            Self::C3D10M => "C3D10M",
        }
    }

    /// Number of nodes an element of this type lists.
    pub fn node_count(self) -> usize {
        match self {
            Self::C3D8I | Self::C3D8 => 8,
            Self::C3D10 | Self::C3D10M => 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub id: u32,
    pub kind: ElementKind,
    pub nodes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub elastic: Option<Elastic>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elastic {
    pub young: f64,
    pub poisson: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub name: Option<String>,
    pub nlgeom: bool,
    pub statics: Option<Static>,
    pub boundaries: Vec<Boundary>,
    pub cloads: Vec<Cload>,
}

/// The data line of `*STATIC`. Each value is optional.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Static {
    pub initial_inc: Option<f64>,
    pub period: Option<f64>,
    pub min_inc: Option<f64>,
    pub max_inc: Option<f64>,
}

/// A node id or a set name. Set names are not resolved at parse time.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Node(u32),
    Set(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    pub target: Target,
    pub first_dof: u8,
    pub last_dof: u8,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cload {
    pub target: Target,
    pub dof: u8,
    pub magnitude: f64,
}

/// A parse error. `line` is 1-based. `keyword` is the keyword the line
/// belongs to, such as `*ELEMENT`, or `<none>` before the first keyword.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub keyword: String,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, {}: {}", self.line, self.keyword, self.message)
    }
}

impl std::error::Error for ParseError {}
