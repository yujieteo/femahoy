/// The Abaqus-shaped model for the linear static subset.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub elements: Vec<Element>,
    pub nsets: Vec<NodeSet>,
    pub elsets: Vec<ElementSet>,
    pub materials: Vec<Material>,
    /// `*BOUNDARY` data given before the first step.
    pub boundaries: Vec<Boundary>,
    pub sections: Vec<SolidSection>,
    pub element_surfaces: Vec<ElementSurface>,
    /// `*INITIAL CONDITIONS, TYPE=TEMPERATURE`: the start temperature of nodes.
    pub initial_temperatures: Vec<Temperature>,
    pub steps: Vec<Step>,
}

/// `*SOLID SECTION`: a material for an element set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolidSection {
    pub elset: String,
    pub material: String,
}

/// `*SURFACE, TYPE=ELEMENT`: a named list of element faces. A face number is 1 to 4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSurface {
    pub name: String,
    /// `(element id, face number)`, in input order.
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
    pub element_type: String,
    pub nodes: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSet {
    pub name: String,
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSet {
    pub name: String,
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub elastic: Option<Elastic>,
    /// `*EXPANSION`: the isotropic thermal expansion coefficient.
    pub expansion: Option<f64>,
}

/// Isotropic elasticity.
#[derive(Debug, Clone, PartialEq)]
pub struct Elastic {
    pub youngs_modulus: f64,
    pub poissons_ratio: f64,
}

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

/// One data line of `*INITIAL CONDITIONS, TYPE=TEMPERATURE` or `*TEMPERATURE`.
#[derive(Debug, Clone, PartialEq)]
pub struct Temperature {
    pub target: Target,
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

/// One `*DLOAD` data line: a pressure on face `face` (1 to 4) of the elements.
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

/// A `*STEP` with its `*STATIC` procedure. `*STATIC` takes no data line.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub name: Option<String>,
    pub boundaries: Vec<Boundary>,
    pub cloads: Vec<Cload>,
    pub dloads: Vec<Dload>,
    pub dsloads: Vec<Dsload>,
    /// `*TEMPERATURE`: the temperature of nodes at the end of the step.
    pub temperatures: Vec<Temperature>,
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

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}
