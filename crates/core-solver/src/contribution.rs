//! Parts of the system that are not elements. Contact (M6) is one.
//!
//! A contribution adds a force and a tangent to the system at the current positions,
//! like an element, but it is not an element: it has no material and no section, and
//! it keeps its own state with a committed and a trial copy. Its work is cut into
//! groups. A group couples a few nodes, for example a slave node and the nodes of a
//! master face in node-to-surface contact. The matrix pattern gets a block for each
//! group, as for an element, and the assembly adds a group through the same slot map.
//! Decision D-10, see DESIGN.md.
//!
//! The groups are fixed for a run in M2. A contact search that changes the groups at an
//! increment (M6) rebuilds the node graph and the pattern, as a change of the constraint
//! structure does.

use std::fmt;
use std::sync::Arc;

use fh_inp::Model;

use crate::element::{Evaluation, Placement, MAX_MAT_STATE, MAX_POINTS};
use crate::SolverError;

/// Largest number of nodes in one group. A group uses the translations of its nodes.
pub const MAX_GROUP_NODES: usize = 10;
/// Largest number of state numbers of one group.
pub const MAX_GROUP_STATE: usize = MAX_POINTS * MAX_MAT_STATE;

/// A force and tangent contribution that is not an element.
pub trait Contribution: Send + Sync {
    /// A name for messages, for example the name of the contact pair.
    fn name(&self) -> &str;

    /// The node labels (the numbers of the input file) of each group. The solver asks
    /// once, before it builds the matrix pattern.
    fn groups(&self) -> Vec<Vec<u32>>;

    /// Numbers of state of each group. The restart file keeps them.
    fn state_len(&self) -> usize {
        0
    }

    /// The internal force of group `group` at the current positions (`place`), and with
    /// `tangent` its matrix, added into `out` in the layout of an element evaluation
    /// (`out.fint`, `out.fabs`, `out.k`, with `out.n` dofs: 3 for each node, in the order of
    /// the group). `fabs` is the sum of the absolute forces, a scale for the Newton check.
    /// The solver clears `out` before the call. `old` is the committed state. The trial state
    /// is in `Evaluation::state_mut` and starts as a copy of `old`. An error rejects the trial,
    /// and the step cuts the increment back. The call must not allocate: it is a pure kernel.
    fn evaluate(
        &self,
        group: usize,
        place: &Placement,
        old: &[f64],
        out: &mut Evaluation,
        tangent: bool,
    ) -> Result<(), SolverError>;
}

type Build = dyn Fn(&Model) -> Result<Vec<Box<dyn Contribution>>, SolverError> + Send + Sync;

/// Builds the contributions of a model. M6 adds contact from `*CONTACT PAIR` here.
#[derive(Clone)]
pub struct ContributionFactory(Arc<Build>);

impl ContributionFactory {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&Model) -> Result<Vec<Box<dyn Contribution>>, SolverError> + Send + Sync + 'static,
    {
        ContributionFactory(Arc::new(f))
    }

    pub fn build(&self, model: &Model) -> Result<Vec<Box<dyn Contribution>>, SolverError> {
        (self.0)(model)
    }
}

impl fmt::Debug for ContributionFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContributionFactory")
    }
}
