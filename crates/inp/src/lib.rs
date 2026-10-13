//! `.inp` reader and writer, Abaqus-shaped model, template mapping.
//! Decision D-05, see DESIGN.md: the model input is Abaqus-style `.inp`.
//!
//! M1 start: the reader for the linear static keyword subset.

mod error;
mod lexer;
mod model;
mod parse;

pub use error::{code, join_errors, ParseError};
pub use model::{
    Boundary, Cload, Coupling, CouplingKind, Dload, Dsload, Elastic, Element, ElementSet,
    ElementSurface, ElementTarget, Equation, EquationTerm, Instance, Material, Model, Mpc, MpcKind,
    Node, NodeSet, NodeSurface, Origin, SolidSection, Static, Step, Target,
};
pub use parse::{parse, parse_all, parse_file};
