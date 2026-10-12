//! `.inp` reader and writer, Abaqus-shaped model, template mapping.
//!
//! M1 start: the reader for the linear static keyword subset.

mod error;
mod lexer;
mod model;
mod parse;

pub use error::ParseError;
pub use model::{
    Boundary, Cload, Elastic, Element, ElementSet, Material, Model, Node, NodeSet, Static, Step,
    Target,
};
pub use parse::parse;
