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
    pub surfaces: Vec<NodeSurface>,
    pub element_surfaces: Vec<ElementSurface>,
    pub equations: Vec<Equation>,
    pub mpcs: Vec<Mpc>,
    pub couplings: Vec<Coupling>,
    pub steps: Vec<Step>,
}

/// `*SOLID SECTION`: a material for an element set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolidSection {
    pub elset: String,
    pub material: String,
}

/// `*SURFACE, TYPE=NODE`: a named list of nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSurface {
    pub name: String,
    pub ids: Vec<u32>,
}

/// `*SURFACE, TYPE=ELEMENT`: a named list of element faces. A face number is 1 to 6
/// (1 to 4 for a tet). The nodes of a face depend on the element type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementSurface {
    pub name: String,
    /// `(element id, face number)`, in input order.
    pub faces: Vec<(u32, u8)>,
}

/// One term of an `*EQUATION`: `coefficient * u(node, dof)`.
#[derive(Debug, Clone, PartialEq)]
pub struct EquationTerm {
    pub node: u32,
    pub dof: u8,
    pub coefficient: f64,
}

/// `*EQUATION`: the sum of the terms is zero.
#[derive(Debug, Clone, PartialEq)]
pub struct Equation {
    pub terms: Vec<EquationTerm>,
}

/// `*MPC`: a type name and the nodes it joins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mpc {
    pub kind: MpcKind,
    pub nodes: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpcKind {
    /// All translations of both nodes are equal.
    Tie,
    /// Same as `Tie` for solid nodes.
    Pin,
    /// The distance between two nodes stays constant (first order).
    Link,
    /// Rigid beam: the second node follows the first, with rotation.
    Beam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CouplingKind {
    /// Slave nodes follow the reference node rigidly (RBE2 behaviour).
    Kinematic,
    /// Reference node motion is the weighted average of the slave nodes (RBE3 behaviour).
    Distributing,
}

/// `*COUPLING` with `*KINEMATIC` or `*DISTRIBUTING`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coupling {
    pub name: String,
    pub ref_node: u32,
    pub surface: String,
    pub kind: CouplingKind,
    pub first_dof: u8,
    pub last_dof: u8,
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

/// An element number or an element set name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementTarget {
    Element(u32),
    Set(String),
}

/// One `*DLOAD` data line: a pressure on face `face` (1 to 6) of the elements.
#[derive(Debug, Clone, PartialEq)]
pub struct Dload {
    pub target: ElementTarget,
    pub face: u8,
    pub pressure: f64,
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
    pub dloads: Vec<Dload>,
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
        self.element_surfaces.iter().find(|s| s.name == name)
    }

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
    }
}
