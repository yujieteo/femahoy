/// The parts of a CalculiX input that the preview draws: the mesh, the sets, the surfaces,
/// the supports and the loads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub nsets: Vec<NodeSet>,
    pub elsets: Vec<ElementSet>,
    /// `*BOUNDARY` data given before the first step.
    pub boundaries: Vec<Boundary>,
    pub element_surfaces: Vec<ElementSurface>,
    pub steps: Vec<Step>,
}

/// `*SURFACE`: a named list of element faces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSurface {
    pub name: String,
    /// `(element id, face number)`, in input order. A shell has the faces 1 (`SPOS`) and 2
    /// (`SNEG`).
    pub faces: Vec<(u32, u8)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: u32,
    pub xyz: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub id: u32,
    /// The upper case type, for example `C3D10` or `S8R`.
    pub element_type: String,
    pub nodes: Vec<u32>,
}

/// A named list of node ids or element ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdSet {
    pub name: String,
    pub ids: Vec<u32>,
}

pub type NodeSet = IdSet;
pub type ElementSet = IdSet;

/// A node number or a node set name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Node(u32),
    Set(String),
}

/// One `*BOUNDARY` data line. The value is 0 when the line has none.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    pub target: Target,
    pub first_dof: u8,
    pub last_dof: u8,
    pub value: f64,
}

/// One `*CLOAD` data line.
#[derive(Debug, Clone, PartialEq)]
pub struct Cload {
    pub target: Target,
    pub dof: u8,
    pub magnitude: f64,
}

/// An element number or an element set name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementTarget {
    Element(u32),
    Set(String),
}

/// One `*DLOAD` data line: a pressure on face `face` (1 to 6) of the elements. A shell has
/// the load type `P` with no number, and `face` is 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Dload {
    pub target: ElementTarget,
    pub face: u8,
    pub pressure: f64,
}

/// One `*DSLOAD` data line: a pressure on every face of an element surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Dsload {
    pub surface: String,
    pub pressure: f64,
}

/// A `*STEP` with its supports and loads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub name: Option<String>,
    pub boundaries: Vec<Boundary>,
    pub cloads: Vec<Cload>,
    pub dloads: Vec<Dload>,
    pub dsloads: Vec<Dsload>,
}

impl Model {
    pub fn nset(&self, name: &str) -> Option<&NodeSet> {
        self.nsets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn elset(&self, name: &str) -> Option<&ElementSet> {
        self.elsets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn element_surface(&self, name: &str) -> Option<&ElementSurface> {
        self.element_surfaces
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }
}
