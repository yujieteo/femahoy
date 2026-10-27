//! The built-in finite element solver of femahoy. It has no I/O and no dependency, so it compiles
//! to wasm for the browser and natively for `fh run`.
//!
//! - `grid`: the graded box grid of a list of parts (boxes).
//! - `sparse`, `order`, `symbolic`, `cholesky`: a sparse symmetric matrix, a fill-reducing
//!   ordering, the symbolic analysis of the factor and the numeric real Cholesky factor. The
//!   ordering and the symbolic analysis use only the pattern, so another numeric factor (for
//!   example a complex LDL^T) can use them.
//! - `multigrid`: conjugate gradients with a geometric multigrid preconditioner on the grid, for
//!   1 or 3 unknowns at each point.
//! - `solve`: the choice between the direct solver and PCG.
//! - `heat`: steady and transient heat conduction on 8-node hex elements.
//! - `hex`: the 8-node hex with incompatible modes for elasticity, and the orthotropic material.
//! - `elastic`: linear static thermo-elasticity on the grid.

// `!(a > b)` is also true for NaN, so the checks of the input and of each pivot use it. The
// numeric loops index several arrays with 1 index, as the formulas of the sources do.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

pub mod cholesky;
pub mod elastic;
pub mod grid;
pub mod heat;
pub mod hex;
pub mod multigrid;
pub mod order;
pub mod solve;
pub mod sparse;
pub mod symbolic;
