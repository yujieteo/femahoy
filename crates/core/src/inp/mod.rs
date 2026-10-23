//! A small reader of CalculiX `.inp` text for the preview of supports and loads, and the files
//! of an input (`*INCLUDE`, and the global input of `*SUBMODEL`). CalculiX reads and checks the
//! input itself. The reader takes the mesh, the sets, the surfaces, `*BOUNDARY`, `*CLOAD`,
//! `*DLOAD` and `*DSLOAD`, and skips every other keyword with no error.

mod files;
mod lexer;
mod model;
mod parse;

pub use files::{flatten, global_inputs, include_path, includes, units, Flat};
pub use model::{
    Boundary, Cload, Dload, Dsload, Element, ElementSet, ElementSurface, ElementTarget, IdSet,
    Model, Node, NodeSet, Step, Target,
};
pub use parse::parse;
