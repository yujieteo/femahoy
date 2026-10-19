//! `.inp` reader and Abaqus-shaped model.
//! Decision D-05, see DESIGN.md: the model input is Abaqus-style `.inp`.
//!
//! The reader for the linear static keyword subset.

mod error;
mod lexer;
mod model;
mod parse;

pub use error::{code, join_errors, ParseError};
pub use model::{
    Boundary, Cload, Dload, Dsload, Elastic, Element, ElementSet, ElementSurface, ElementTarget,
    Material, Model, Node, NodeSet, SolidSection, Step, Target, Temperature,
};
pub use parse::{parse, parse_all, parse_file};
