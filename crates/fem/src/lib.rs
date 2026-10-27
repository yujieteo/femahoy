//! The built-in finite element solver of femahoy. It has no I/O and no dependency, so it compiles
//! to wasm for the browser and natively for `fh run`.
//!
//! - `grid`: the graded box grid of a list of parts (boxes).
//! - `predicates`, `mesh2d`, `prism`: robust orientation and in-circle tests, the quality
//!   Delaunay triangle mesh of the outlines of the parts, and its extrusion along 1 axis into
//!   tetrahedra, for parts that are prisms of polygons and circles.
//! - `sparse`, `order`, `symbolic`, `cholesky`: a sparse symmetric matrix, a fill-reducing
//!   ordering, the symbolic analysis of the factor and the numeric real Cholesky factor. The
//!   ordering and the symbolic analysis use only the pattern, so another numeric factor (for
//!   example a complex LDL^T) can use them.
//! - `complex`, `ldlt`: complex numbers and small dense complex matrices, and the complex
//!   symmetric sparse LDL^T factor on the ordering and the symbolic analysis of the real Cholesky
//!   factor.
//! - `line`, `mtl`: the per-length R, L, G and C of a transmission line from its cross-section
//!   (electrostatic and magneto-quasi-static solves on the triangle mesh), and the chain matrix,
//!   the S matrix, the modes and the Touchstone text of a multiconductor line.
//! - `multigrid`: conjugate gradients with a geometric multigrid preconditioner on the grid, for
//!   1 or 3 unknowns at each point.
//! - `solve`: the choice between the direct solver and PCG.
//! - `heat`: steady and transient heat conduction on 8-node hex elements on the grid, or on
//!   4-node tetrahedra on the extruded mesh.
//! - `hex`: the 8-node hex with incompatible modes for elasticity, and the orthotropic material.
//! - `elastic`: linear static thermo-elasticity on the grid, and `elastic_tet` on the extruded
//!   mesh with 10-node tetrahedra (`tet10`).
//! - `pmultigrid`: the p-multigrid preconditioner of the 10-node tetrahedra, with the linear
//!   tetrahedra as the coarse level.
//! - `homog`: the effective conductivity, stiffness and expansion of a periodic unit cell.
//! - `em`: frequency-domain electromagnetics with edge elements on the extruded mesh: wave
//!   ports and S-parameters over a frequency sweep, walls, absorbing layers, resonant modes.
//! - `hx`: GMRES with the nodal auxiliary space preconditioner, the iterative solver of `em` for
//!   a model too large for the direct factor.

// `!(a > b)` is also true for NaN, so the checks of the input and of each pivot use it. The
// numeric loops index several arrays with 1 index, as the formulas of the sources do.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

pub mod cholesky;
pub mod complex;
pub mod elastic;
pub mod elastic_tet;
pub mod em;
pub mod grid;
pub mod heat;
pub mod hex;
pub mod homog;
pub mod hx;
pub mod ldlt;
pub mod line;
pub mod mesh2d;
pub mod mtl;
pub mod multigrid;
pub mod order;
pub mod pmultigrid;
pub mod predicates;
pub mod prism;
pub mod solve;
pub mod sparse;
pub mod symbolic;
pub mod tet10;
