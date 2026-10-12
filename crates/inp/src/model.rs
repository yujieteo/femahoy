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
    pub steps: Vec<Step>,
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

/// One `*CLOAD` data line.
#[derive(Debug, Clone, PartialEq)]
pub struct Cload {
    pub target: Target,
    pub dof: u8,
    pub magnitude: f64,
}

/// `*STATIC` data: initial increment, time period, minimum and maximum increment.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Static {
    pub initial_increment: Option<f64>,
    pub time_period: Option<f64>,
    pub min_increment: Option<f64>,
    pub max_increment: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub name: Option<String>,
    pub nlgeom: bool,
    pub static_procedure: Option<Static>,
    pub boundaries: Vec<Boundary>,
    pub cloads: Vec<Cload>,
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

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}
