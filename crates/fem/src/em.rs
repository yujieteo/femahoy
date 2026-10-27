//! Frequency-domain electromagnetics on the 2.5D tetrahedral mesh (`prism`): the electric field
//! E of the time-harmonic Maxwell equations with first-order edge elements, driven by wave ports
//! at a sweep of frequencies (S-parameters), or the resonant modes of a closed model.
//!
//! The time factor is exp(j omega t). The unknowns are the line integrals of E along the edges
//! of the mesh (the Whitney functions W_ij = l_i grad l_j - l_j grad l_i of the barycentric
//! coordinates l of each tetrahedron), and the equation is the curl-curl equation
//!
//!   curl (1/mu_r curl E) - k0^2 eps_r E = 0,  eps_r = eps' (1 - j tan d) - j sigma / (omega eps0).
//!
//! Its weak form gives the matrix S - k0^2 M + the face terms, complex symmetric. The boundary
//! conditions are on named face sets:
//!
//! - an electric wall (a perfect conductor, n x E = 0): the edges on it are fixed at 0. An outer
//!   face with no wall or port, and a face between the mesh and a perfect conductor part (a
//!   hollow part with no mesh), is an electric wall;
//! - a magnetic wall (n x H = 0): the natural condition of the weak form, no term;
//! - a good conductor: the Leontovich surface impedance Zs = (1 + j) / (sigma delta) with the
//!   skin depth delta = sqrt(2 / (omega mu0 sigma)), the face term j omega mu0 / Zs E_t . W_t;
//! - a wave port: the condition n x curl E + j beta n x n x E = -2 j beta E_inc,t of 1 mode with
//!   the propagation constant beta, which absorbs the outgoing wave of that mode. The mode is
//!   TE10 of a rectangular waveguide (exact), or the quasi-TEM mode of a line, from 2
//!   electrostatic solves on the triangles of the port face (the signal at 1 V, the electric
//!   walls at 0 V, with eps_r and with 1: eps_eff = C / C0, beta = k0 sqrt(eps_eff),
//!   Z0 = sqrt(eps_eff) / (c C)).
//!
//! An absorbing layer (PML) is a part with complex stretched coordinates: s = 1 - j a (r / d)^2
//! along each axis on which the part is outside the box of the other parts, with the depth d and
//! a = 3 ln(1 / R) / (2 k d) for a reflection R at normal incidence. The tensors eps_r Lambda
//! and (mu_r Lambda)^-1 with Lambda = diag(sy sz / sx, sx sz / sy, sx sy / sz) hold the stretch.
//!
//! Each port has a mode with the power 1/(2 Zw) |a|^2 for the amplitude a, Zw = omega mu / beta.
//! The excitation of port j is a wave of 1 W. The amplitude of the wave out of port i is
//! b_i = (E_t, e_i) - a_j delta_ij, and S_ij = (b_i / sqrt(Zw_i)) / (a_j / sqrt(Zw_j)): the
//! generalized S-parameters, each port normalized to its own mode. A shift d_i of the reference
//! plane of port i into the model gives S_ij exp(j beta_i d_i + j beta_j d_j).
//!
//! The resonant modes of a closed model solve S x = k^2 M x near a target k_t^2 by subspace
//! iteration with the shifted and inverted operator (S - k_t^2 M)^-1 M and a Rayleigh-Ritz step.
//! The face terms of good conductors and the losses of the materials change with the frequency,
//! so each mode solves again with the terms at its own frequency. A mode has the frequency Re(f)
//! and Q = Re(k) / (2 Im(k)).
//!
//! A frequency sweep solves each frequency with the direct LDL^T factor (`ldlt`) up to the limit
//! `direct` of the points, and with GMRES and the auxiliary space preconditioner (`hx`) above it.
//! A mode solve always uses the direct factor.
//!
//! Sources: J.-C. Nedelec, Mixed finite elements in R3, Numer. Math. 35 (1980) 315-341; A.
//! Bossavit, Computational Electromagnetism, Academic Press 1998, chapter 5 (Whitney elements);
//! J.-M. Jin, The Finite Element Method in Electromagnetics, 3rd ed., Wiley 2014, chapters 8
//! (edge elements, the curl-curl weak form, waveguide ports and S-parameters) and 9 (absorbing
//! boundaries and perfectly matched layers); W. C. Chew and W. H. Weedon, A 3D perfectly matched
//! medium from modified Maxwell's equations with stretched coordinates, Microw. Opt. Technol.
//! Lett. 7 (1994) 599-604; T. B. A. Senior, Impedance boundary conditions for imperfectly
//! conducting surfaces, Appl. Sci. Res. B 8 (1960) 418-436; D. M. Pozar, Microwave Engineering,
//! 4th ed., Wiley 2012, sections 2.7 (surface resistance), 3.3 (TE10), 3.4 (coaxial line),
//! 4.3 (S-parameters, generalized S-parameters and the shift of reference planes) and 6.3
//! (cavity resonators); Y. Saad, Numerical Methods for Large Eigenvalue Problems, 2nd ed., SIAM
//! 2011, chapters 4 and 5 (subspace iteration, shift and invert, Rayleigh-Ritz).

use std::collections::{HashMap, HashSet};

use crate::cholesky::Cholesky;
use crate::complex::{self, Matrix, C64};
use crate::grid::{Part, Spacing};
use crate::heat::{face_tol, selects, tet_gradients, Cell, FaceSet, Mesh};
use crate::hx;
use crate::ldlt::Ldlt;
use crate::order::nested_dissection;
use crate::prism;
use crate::sparse::{Csr, Zcsr};
use crate::symbolic::Symbolic;

/// The speed of light in vacuum (m/s).
pub const LIGHT: f64 = 299_792_458.0;
/// The permeability of vacuum (H/m), CODATA 2018.
pub const MU0: f64 = 1.256_637_062_12e-6;
/// The permittivity of vacuum (F/m).
pub const EPS0: f64 = 1.0 / (MU0 * LIGHT * LIGHT);

/// The reflection of an absorbing layer at normal incidence, for the stretch of its profile.
/// Reason: the test `an_absorbing_layer_ends_a_waveguide_with_a_small_reflection` measured
/// |S11| = 0.0034 with 1e-6 and 0.0031 with 1e-4 at 10 cells across the layer; the mesh, not
/// the profile, sets the reflection, and a smaller value makes the profile steeper.
const PML_REFLECTION: f64 = 1e-4;

/// A material. A perfect conductor is a hollow part: the mesh has no tetrahedra in it, and its
/// faces are electric walls.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The relative permittivity (the real part).
    pub permittivity: f64,
    pub loss_tangent: f64,
    /// The relative permeability.
    pub permeability: f64,
    /// The electric conductivity (S/m).
    pub conductivity: f64,
    pub perfect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wall {
    Electric,
    Magnetic,
    /// A good conductor of this conductivity (S/m).
    Conductor(f64),
}

/// A boundary condition on the faces of a face set.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    pub faces: usize,
    pub wall: Wall,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// TE10 of a rectangular waveguide: the port face is a rectangle with a uniform fill.
    Te10,
    /// The quasi-TEM mode of a line, with the signal conductor on the faces of this face set.
    Tem { signal: usize },
}

/// A wave port. Port k of the S-matrix is the item k of `Model::ports`.
#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub faces: usize,
    pub mode: Mode,
    /// The shift of the reference plane into the model, in the length unit of the model.
    pub deembed: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// The S-parameters at `points` frequencies from `start` to `stop` (Hz), with the fields of
    /// every `output_every`-th frequency and the last.
    Frequency {
        start: f64,
        stop: f64,
        points: usize,
        output_every: usize,
    },
    /// The `count` resonant modes nearest to the frequency `near` (Hz).
    Modes { near: f64, count: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub spacing: Spacing,
    pub axis: usize,
    /// The length of 1 unit of the coordinates in m (1 for SI-m, 0.001 for SI-mm).
    pub length: f64,
    /// True for an absorbing layer (PML), 1 item for each part.
    pub pml: Vec<bool>,
    pub faces: Vec<FaceSet>,
    pub walls: Vec<Boundary>,
    pub ports: Vec<Port>,
    pub step: Step,
}

/// The wave of a port at 1 frequency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortWave {
    /// The propagation constant (rad/m).
    pub beta: f64,
    /// The characteristic impedance Z0 of a TEM port, the wave impedance of a TE10 port (ohm).
    pub impedance: f64,
}

/// The result at 1 frequency, or 1 mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub frequency: f64,
    /// E at the centre of each cell: for each port, with a wave of 1 W into that port; for a
    /// mode, 1 field with a largest magnitude of 1. Empty when this frequency has no field output.
    pub fields: Vec<Vec<[C64; 3]>>,
    /// The S-matrix, row-major, ports x ports.
    pub s: Vec<C64>,
    pub ports: Vec<PortWave>,
    /// The quality factor of a mode.
    pub q: Option<f64>,
}

pub trait Events {
    /// True stops the solve.
    fn cancelled(&self) -> bool {
        false
    }
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String>;
    fn progress(&mut self, text: &str);
    fn frame(&mut self, frame: &Frame) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub cells: usize,
    pub points: usize,
    pub unknowns: usize,
    pub warnings: Vec<String>,
}

/// The most points of the mesh, and the memory that they need, for the error of a larger model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    pub points: usize,
    pub memory: &'static str,
    /// The most points of the direct solver. A larger frequency sweep uses the iterative
    /// solver; a larger mode solve is refused.
    pub direct: usize,
}

/// The limits of `fh run`. Reason for `direct`: the direct solve of the waveguide with a PTFE
/// section of 39,818 points took 65 s, with a factor of 135,040,589 entries (2.2 GB); the
/// iterative solve took 35 s and 1.16 GB. The direct solver stays the first choice up to this
/// size, because the iterative solver needs many more iterations on some models (872 against
/// 122 on the coaxial line of `tests/em.rs`). Reason for `points`: the iterative solve of
/// 122,244 points took 306 s and 4.3 GB. The time of the iterative solver is not a function of
/// the points only: its iterations grow with the length of the model in wavelengths (175 on a
/// WR-90 guide 30 mm long, 381 on a guide 60 mm long, both with max_cell 1 mm), and the guide
/// 60 mm long with 88,479 points took 348 iterations and 564 s of CPU. See the ignored tests
/// `direct_cost` and `iterative_cost` of `tests/em.rs`.
pub const LAPTOP: Limit = Limit {
    points: 100_000,
    memory: "the iterative solver solves in about 4 GB of memory",
    direct: 40_000,
};

/// The local edges of a tetrahedron.
const EDGES: [[usize; 2]; 6] = [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]];

/// What a boundary face is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Electric,
    Magnetic,
    Conductor(f64),
    Port(usize),
}

/// A boundary face with its 3 edges and the matrix of the tangential Whitney functions,
/// the integral of W_i . W_j over the face.
struct FaceEdges {
    edges: [u32; 3],
    mass: [f64; 9],
}

/// The mode of a port, with no frequency in it.
enum PortMode {
    Te10 {
        /// The cutoff wavenumber pi / a (rad/m).
        kc: f64,
        eps: f64,
        mu: f64,
    },
    Tem {
        eps_eff: f64,
        /// The capacitance per length over eps0.
        cap: f64,
    },
}

struct PortData {
    faces: Vec<FaceEdges>,
    /// The integral of W_e . e over the port for each edge e, e the mode with a norm of 1.
    proj: Vec<(u32, f64)>,
    mode: PortMode,
    /// The shift of the reference plane (m).
    deembed: f64,
}

impl PortData {
    /// The wave of the port at the angular frequency `omega`, and mu_r of the fill.
    fn wave(&self, omega: f64, k: usize) -> Result<(PortWave, f64), String> {
        let k0 = omega / LIGHT;
        match self.mode {
            PortMode::Te10 { kc, eps, mu } => {
                let b2 = k0 * k0 * eps * mu - kc * kc;
                if !(b2 > 0.0) {
                    return Err(format!(
                        "port {}: the frequency {:.6e} Hz is below the cutoff {:.6e} Hz of its \
                         TE10 mode, so the mode does not carry power",
                        k + 1,
                        omega / (2.0 * std::f64::consts::PI),
                        kc * LIGHT / (2.0 * std::f64::consts::PI * (eps * mu).sqrt())
                    ));
                }
                let beta = b2.sqrt();
                let zw = omega * MU0 * mu / beta;
                Ok((
                    PortWave {
                        beta,
                        impedance: zw,
                    },
                    mu,
                ))
            }
            PortMode::Tem { eps_eff, cap } => {
                let beta = k0 * eps_eff.sqrt();
                Ok((
                    PortWave {
                        beta,
                        impedance: eps_eff.sqrt() / (LIGHT * EPS0 * cap),
                    },
                    1.0,
                ))
            }
        }
    }
}

/// The stretch of an absorbing layer along 1 axis: the plane where it starts and its depth (m).
#[derive(Debug, Clone, Copy)]
struct Stretch {
    origin: f64,
    depth: f64,
}

/// The model on its mesh.
struct Discrete {
    mesh: Mesh,
    /// The points in m.
    points: Vec<[f64; 3]>,
    tets: Vec<[u32; 4]>,
    /// The gradients of the barycentric coordinates of each tetrahedron (1/m) and its volume.
    grads: Vec<[[f64; 3]; 4]>,
    volume: Vec<f64>,
    /// The global edge of each local edge, and its local points with the smaller global point
    /// first (the direction of the edge).
    tet_edges: Vec<[u32; 6]>,
    tet_pairs: Vec<[[u8; 2]; 6]>,
    /// The unknown of each edge, or `u32::MAX` for an edge on an electric wall.
    unknown: Vec<u32>,
    unknowns: usize,
    /// The stretch of each tetrahedron of an absorbing layer, along x, y and z.
    stretch: Vec<Option<[Option<Stretch>; 3]>>,
    /// The faces of good conductors, with the conductivity.
    conductors: Vec<(FaceEdges, f64)>,
    /// True for a point on an electric wall or a good conductor.
    walled: Vec<bool>,
    /// The 2 points of each edge, the smaller first.
    edges: Vec<[u32; 2]>,
    ports: Vec<PortData>,
    pattern: Csr,
    /// The symbolic factor of the direct solver, or `None` for the iterative solver.
    sym: Option<Symbolic>,
    warnings: Vec<String>,
}

/// Solve the model on a laptop.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Summary, String> {
    solve_within(model, LAPTOP, events)
}

/// Solve the model, and refuse a mesh with more points than `limit`.
pub fn solve_within(
    model: &Model,
    limit: Limit,
    events: &mut dyn Events,
) -> Result<Summary, String> {
    check(model)?;
    let d = discretize(model, limit, events)?;
    let summary = Summary {
        cells: d.tets.len(),
        points: d.points.len(),
        unknowns: d.unknowns,
        warnings: d.warnings.clone(),
    };
    match model.step {
        Step::Frequency {
            start,
            stop,
            points,
            output_every,
        } => sweep(model, &d, start, stop, points, output_every, events)?,
        Step::Modes { near, count } => modes(model, &d, near, count, events)?,
    }
    Ok(summary)
}

fn check(model: &Model) -> Result<(), String> {
    for m in &model.materials {
        let ok = m.permittivity.is_finite()
            && m.permittivity > 0.0
            && m.permeability.is_finite()
            && m.permeability > 0.0
            && m.loss_tangent.is_finite()
            && m.loss_tangent >= 0.0
            && m.conductivity.is_finite()
            && m.conductivity >= 0.0;
        if !ok {
            return Err(format!(
                "material {}: the permittivity and the permeability must be above 0, the loss \
                 tangent and the conductivity 0 or more",
                m.name
            ));
        }
    }
    if model.pml.len() != model.parts.len() {
        return Err("each part needs its pml flag".into());
    }
    for (p, &pml) in model.parts.iter().zip(&model.pml) {
        if pml && model.materials[p.material].perfect {
            return Err(format!(
                "part {}: an absorbing layer cannot be a perfect conductor",
                p.name
            ));
        }
    }
    if !(model.length > 0.0 && model.length.is_finite()) {
        return Err("the length unit must be above 0".into());
    }
    for w in &model.walls {
        if let Wall::Conductor(s) = w.wall {
            if !(s.is_finite() && s > 0.0) {
                return Err(format!(
                    "face set {}: a conductor wall needs a conductivity above 0",
                    model.faces[w.faces].name
                ));
            }
        }
    }
    for (k, p) in model.ports.iter().enumerate() {
        if !p.deembed.is_finite() {
            return Err(format!("port {}: deembed must be a finite number", k + 1));
        }
    }
    match model.step {
        Step::Frequency {
            start,
            stop,
            points,
            output_every,
        } => {
            if !(start > 0.0 && start.is_finite() && stop.is_finite() && stop >= start) {
                return Err("a frequency step needs 0 < start <= stop".into());
            }
            if points == 0 || output_every == 0 {
                return Err("points and output_every must be 1 or more".into());
            }
            if points > 1 && stop == start {
                return Err("a sweep of 2 or more points needs stop above start".into());
            }
            if model.ports.is_empty() {
                return Err(
                    "a frequency step needs a port: the S-parameters are the waves of the ports"
                        .into(),
                );
            }
        }
        Step::Modes { near, count } => {
            if !(near > 0.0 && near.is_finite()) {
                return Err("a modes step needs near above 0".into());
            }
            if !(1..=20).contains(&count) {
                return Err("a modes step finds 1 to 20 modes".into());
            }
            if !model.ports.is_empty() || model.pml.iter().any(|&p| p) {
                return Err(
                    "a modes step solves a closed model: remove the ports and the absorbing \
                     layers"
                        .into(),
                );
            }
        }
    }
    Ok(())
}

/// The barycentric gradients in the plane of a triangle and its area.
fn face_gradients(p: [[f64; 3]; 3]) -> ([[f64; 3]; 3], f64) {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let twice = dot3(n, n).sqrt();
    let unit = n.map(|v| v / twice);
    let g = std::array::from_fn(|i| {
        let e = sub(p[(i + 2) % 3], p[(i + 1) % 3]);
        cross(unit, e).map(|v| v / twice)
    });
    (g, 0.5 * twice)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The local edges of a triangle, each with the smaller global point first.
fn face_pairs(nodes: [u32; 3]) -> [[usize; 2]; 3] {
    [[0, 1], [1, 2], [0, 2]].map(|[a, b]| if nodes[a] < nodes[b] { [a, b] } else { [b, a] })
}

/// The integral of W_i . W_j over a triangle for its 3 edges `pairs`. The integral of
/// l_a l_b is A (1 + delta_ab) / 12.
fn face_mass(g: [[f64; 3]; 3], area: f64, pairs: [[usize; 2]; 3]) -> [f64; 9] {
    let m = |a: usize, b: usize| area * if a == b { 2.0 } else { 1.0 } / 12.0;
    let mut out = [0.0; 9];
    for (e, &[i, j]) in pairs.iter().enumerate() {
        for (f, &[k, l]) in pairs.iter().enumerate() {
            out[e * 3 + f] = m(i, k) * dot3(g[j], g[l])
                - m(i, l) * dot3(g[j], g[k])
                - m(j, k) * dot3(g[i], g[l])
                + m(j, l) * dot3(g[i], g[k]);
        }
    }
    out
}

/// The 7-point rule of degree 5 on a triangle: the barycentric coordinates and the weight of
/// each point, a fraction of the area (A. H. Stroud, Approximate Calculation of Multiple
/// Integrals, Prentice-Hall 1971, formula T2:5-1, after J. Radon 1948).
fn triangle_rule() -> [([f64; 3], f64); 7] {
    let s = 15f64.sqrt();
    let (a1, b1) = ((6.0 - s) / 21.0, (9.0 + 2.0 * s) / 21.0);
    let (a2, b2) = ((6.0 + s) / 21.0, (9.0 - 2.0 * s) / 21.0);
    let (w1, w2) = ((155.0 - s) / 1200.0, (155.0 + s) / 1200.0);
    [
        ([1.0 / 3.0; 3], 9.0 / 40.0),
        ([a1, a1, b1], w1),
        ([a1, b1, a1], w1),
        ([b1, a1, a1], w1),
        ([a2, a2, b2], w2),
        ([a2, b2, a2], w2),
        ([b2, a2, a2], w2),
    ]
}

/// The mesh, the edges, the boundary faces, the ports and the symbolic factor of the model.
fn discretize(model: &Model, limit: Limit, events: &mut dyn Events) -> Result<Discrete, String> {
    let refusal = format!(
        "the mesh has more than {} points, the most that {}: use a larger max_cell or a larger \
         max_growth",
        limit.points, limit.memory
    );
    let hollow: Vec<bool> = model
        .parts
        .iter()
        .map(|p| model.materials[p.material].perfect)
        .collect();
    let tm = prism::mesh_hollow(
        &model.parts,
        model.axis,
        model.spacing,
        limit.points,
        &refusal,
        false,
        &hollow,
    )?;
    let mesh = Mesh {
        points: tm.points.clone(),
        kind: Cell::Tet4,
        cells: tm.tets.iter().flatten().copied().collect(),
        part: tm.part.clone(),
    };
    events.mesh(&mesh)?;
    let points: Vec<[f64; 3]> = tm
        .points
        .iter()
        .map(|p| p.map(|v| v * model.length))
        .collect();
    // The edges.
    let mut edge_of: HashMap<(u32, u32), u32> = HashMap::new();
    let mut edges: Vec<[u32; 2]> = Vec::new();
    let mut tet_edges = Vec::with_capacity(tm.tets.len());
    let mut tet_pairs = Vec::with_capacity(tm.tets.len());
    let mut grads = Vec::with_capacity(tm.tets.len());
    let mut volume = Vec::with_capacity(tm.tets.len());
    for tet in &tm.tets {
        let mut ids = [0u32; 6];
        let mut pairs = [[0u8; 2]; 6];
        for (k, &[a, b]) in EDGES.iter().enumerate() {
            let (a, b) = if tet[a] < tet[b] { (a, b) } else { (b, a) };
            let key = (tet[a], tet[b]);
            let next = edges.len() as u32;
            let id = *edge_of.entry(key).or_insert(next);
            if id == next {
                edges.push([key.0, key.1]);
            }
            ids[k] = id;
            pairs[k] = [a as u8, b as u8];
        }
        tet_edges.push(ids);
        tet_pairs.push(pairs);
        let (g, v) = tet_gradients(tet.map(|i| points[i as usize]));
        grads.push(g);
        volume.push(v);
    }
    let edge = |a: u32, b: u32| edge_of[&(a.min(b), a.max(b))];
    // The kind of each boundary face: an electric wall unless a wall or a port names it.
    let t = face_tol(&tm.points);
    let mut kind = vec![Kind::Electric; tm.faces.len()];
    for w in &model.walls {
        let set = &model.faces[w.faces];
        let mut any = false;
        for (k, f) in tm.faces.iter().enumerate() {
            if selects(set, &tm, f, t).is_some() {
                any = true;
                kind[k] = match w.wall {
                    Wall::Electric => Kind::Electric,
                    Wall::Magnetic => Kind::Magnetic,
                    Wall::Conductor(s) => Kind::Conductor(s),
                };
            }
        }
        if !any {
            return Err(empty_set(&set.name));
        }
    }
    for (p, port) in model.ports.iter().enumerate() {
        let set = &model.faces[port.faces];
        let mut any = false;
        for (k, f) in tm.faces.iter().enumerate() {
            if selects(set, &tm, f, t).is_some() {
                any = true;
                if let Kind::Port(other) = kind[k] {
                    return Err(format!(
                        "ports {} and {} share a face: give each port its own faces",
                        other + 1,
                        p + 1
                    ));
                }
                kind[k] = Kind::Port(p);
            }
        }
        if !any {
            return Err(empty_set(&set.name));
        }
    }
    // The edges on electric walls are fixed at 0.
    let mut fixed = vec![false; edges.len()];
    for (f, k) in tm.faces.iter().zip(&kind) {
        if *k == Kind::Electric {
            let n = f.nodes;
            for [a, b] in [[0, 1], [1, 2], [0, 2]] {
                fixed[edge(n[a], n[b]) as usize] = true;
            }
        }
    }
    let mut walled = vec![false; tm.points.len()];
    for (f, k) in tm.faces.iter().zip(&kind) {
        if matches!(k, Kind::Electric | Kind::Conductor(_)) {
            for v in f.nodes {
                walled[v as usize] = true;
            }
        }
    }
    let mut unknown = vec![u32::MAX; edges.len()];
    let mut unknowns = 0usize;
    for (e, &f) in fixed.iter().enumerate() {
        if !f {
            unknown[e] = unknowns as u32;
            unknowns += 1;
        }
    }
    if unknowns == 0 {
        return Err("the model has no unknown: each edge is on an electric wall".into());
    }
    let face_edges = |f: &prism::BoundaryFace| -> FaceEdges {
        let n = f.nodes;
        let (g, area) = face_gradients(n.map(|i| points[i as usize]));
        let pairs = face_pairs(n);
        FaceEdges {
            edges: pairs.map(|[a, b]| edge(n[a], n[b])),
            mass: face_mass(g, area, pairs),
        }
    };
    let conductors: Vec<(FaceEdges, f64)> = tm
        .faces
        .iter()
        .zip(&kind)
        .filter_map(|(f, k)| match k {
            Kind::Conductor(s) => Some((face_edges(f), *s)),
            _ => None,
        })
        .collect();
    let magnetic: HashSet<u32> = tm
        .faces
        .iter()
        .zip(&kind)
        .filter(|(_, k)| **k == Kind::Magnetic)
        .flat_map(|(f, _)| face_pairs(f.nodes).map(|[a, b]| edge(f.nodes[a], f.nodes[b])))
        .collect();
    let mut ports = Vec::new();
    for (p, port) in model.ports.iter().enumerate() {
        let faces: Vec<&prism::BoundaryFace> = tm
            .faces
            .iter()
            .zip(&kind)
            .filter(|(_, k)| **k == Kind::Port(p))
            .map(|(f, _)| f)
            .collect();
        let material = |f: &prism::BoundaryFace| {
            &model.materials[model.parts[tm.part[f.tet as usize] as usize].material]
        };
        let (mode, proj) = match port.mode {
            Mode::Te10 => te10(p, &faces, &points, &edge, &material, &magnetic)?,
            Mode::Tem { signal } => {
                let set = &model.faces[signal];
                let on_signal: Vec<bool> = tm
                    .faces
                    .iter()
                    .map(|f| selects(set, &tm, f, t).is_some())
                    .collect();
                let nodes = |keep: &dyn Fn(usize) -> bool| -> Vec<u32> {
                    (0..tm.faces.len())
                        .filter(|&k| keep(k))
                        .flat_map(|k| tm.faces[k].nodes)
                        .collect()
                };
                let signal_nodes = nodes(&|k| on_signal[k]);
                let ground_nodes = nodes(&|k| !on_signal[k] && kind[k] == Kind::Electric);
                tem(
                    p,
                    &faces,
                    &points,
                    &edge,
                    &material,
                    &signal_nodes,
                    &ground_nodes,
                )?
            }
        };
        let faces: Vec<FaceEdges> = faces.iter().map(|f| face_edges(f)).collect();
        let proj = discrete_mode(p, &faces, proj, &unknown)?;
        ports.push(PortData {
            faces,
            proj,
            mode,
            deembed: port.deembed * model.length,
        });
    }
    // The stretch of the absorbing layers: along each axis on which the part is outside the box
    // of the other parts.
    let mut inner_lo = [f64::INFINITY; 3];
    let mut inner_hi = [f64::NEG_INFINITY; 3];
    let mut part_lo = vec![[f64::INFINITY; 3]; model.parts.len()];
    let mut part_hi = vec![[f64::NEG_INFINITY; 3]; model.parts.len()];
    for (tet, &p) in tm.tets.iter().zip(&tm.part) {
        for &v in tet {
            for d in 0..3 {
                let x = points[v as usize][d];
                let p = p as usize;
                part_lo[p][d] = part_lo[p][d].min(x);
                part_hi[p][d] = part_hi[p][d].max(x);
                if !model.pml[p] {
                    inner_lo[d] = inner_lo[d].min(x);
                    inner_hi[d] = inner_hi[d].max(x);
                }
            }
        }
    }
    let size = (0..3)
        .map(|d| inner_hi[d] - inner_lo[d])
        .fold(0.0, f64::max);
    let mut part_stretch: Vec<Option<[Option<Stretch>; 3]>> = vec![None; model.parts.len()];
    for (p, part) in model.parts.iter().enumerate() {
        if !model.pml[p] || !part_lo[p][0].is_finite() {
            continue;
        }
        let s: [Option<Stretch>; 3] = std::array::from_fn(|d| {
            if part_lo[p][d] >= inner_hi[d] - 1e-9 * size {
                Some(Stretch {
                    origin: inner_hi[d],
                    depth: part_hi[p][d] - inner_hi[d],
                })
            } else if part_hi[p][d] <= inner_lo[d] + 1e-9 * size {
                Some(Stretch {
                    origin: inner_lo[d],
                    depth: inner_lo[d] - part_lo[p][d],
                })
            } else {
                None
            }
        });
        if s.iter().all(Option::is_none) {
            return Err(format!(
                "part {}: an absorbing layer must be outside the box of the other parts along \
                 x, y or z",
                part.name
            ));
        }
        part_stretch[p] = Some(s);
    }
    let stretch = tm.part.iter().map(|&p| part_stretch[p as usize]).collect();
    // The pattern of the unknowns and its symbolic factor, in the nested dissection order of
    // the middles of the edges.
    let mut flat: Vec<u32> = Vec::with_capacity(6 * tm.tets.len());
    let mut ptr = vec![0usize];
    for ids in &tet_edges {
        flat.extend(
            ids.iter()
                .map(|&e| unknown[e as usize])
                .filter(|&u| u != u32::MAX),
        );
        ptr.push(flat.len());
    }
    let pattern = Csr::pattern(
        unknowns,
        (0..tet_edges.len()).map(|k| &flat[ptr[k]..ptr[k + 1]]),
    );
    let mut xyz = vec![[0.0; 3]; unknowns];
    for (e, &[a, b]) in edges.iter().enumerate() {
        if unknown[e] != u32::MAX {
            let (p, q) = (points[a as usize], points[b as usize]);
            xyz[unknown[e] as usize] = std::array::from_fn(|d| 0.5 * (p[d] + q[d]));
        }
    }
    let modes = matches!(model.step, Step::Modes { .. });
    if modes && tm.points.len() > limit.direct {
        return Err(format!(
            "the mesh has {} points: a mode solve needs the direct solver, which solves up to {} \
             points. Use a larger max_cell or a larger max_growth",
            tm.points.len(),
            limit.direct
        ));
    }
    let sym = (tm.points.len() <= limit.direct)
        .then(|| Symbolic::analyse(&pattern, &nested_dissection(&pattern, &xyz)));
    let solver = match &sym {
        Some(sym) => format!("the direct factor has {} entries", sym.entries()),
        None => "the iterative solver (GMRES, auxiliary space preconditioner)".into(),
    };
    events.progress(&format!(
        "mesh {} triangles x {} planes, {} tetrahedra, {} points, {} edges, {} unknowns; {solver}",
        tm.triangles,
        tm.planes,
        tm.tets.len(),
        tm.points.len(),
        edges.len(),
        unknowns,
    ));
    // The longest edge of each material against its wavelength at the highest frequency.
    let f_max = match model.step {
        Step::Frequency { stop, .. } => stop,
        Step::Modes { near, .. } => near,
    };
    let mut worst = 0.0f64;
    for (tet, &p) in tm.tets.iter().zip(&tm.part) {
        let m = &model.materials[model.parts[p as usize].material];
        let wavelength = LIGHT / (f_max * (m.permittivity * m.permeability).sqrt());
        for [a, b] in EDGES {
            let (u, v) = (points[tet[a] as usize], points[tet[b] as usize]);
            let h = (0..3).map(|d| (u[d] - v[d]).powi(2)).sum::<f64>().sqrt();
            worst = worst.max(h / wavelength);
        }
    }
    let mut warnings = Vec::new();
    if worst > 0.125 {
        warnings.push(format!(
            "the longest edge of the mesh is {worst:.2} of a wavelength at {f_max:.4e} Hz: the \
             result needs 8 or more cells in each wavelength. Use a smaller max_cell"
        ));
    }
    Ok(Discrete {
        mesh,
        points,
        tets: tm.tets.clone(),
        grads,
        volume,
        tet_edges,
        tet_pairs,
        unknown,
        unknowns,
        stretch,
        conductors,
        walled,
        edges,
        ports,
        pattern,
        sym,
        warnings,
    })
}

/// The mode `proj` (the integral of W_e . e for each edge e) scaled to the norm 1 of its
/// projection on the edge functions of the port: the L2 projection e_h = sum c_e W_e solves
/// B c = proj with the face matrix B of the free edges, and |e_h|^2 = c . proj. The port then
/// sends and measures the discrete mode, which keeps the power of a lossless model.
fn discrete_mode(
    p: usize,
    faces: &[FaceEdges],
    proj: Vec<(u32, f64)>,
    unknown: &[u32],
) -> Result<Vec<(u32, f64)>, String> {
    let mut local: HashMap<u32, u32> = HashMap::new();
    for f in faces {
        for &e in &f.edges {
            if unknown[e as usize] != u32::MAX {
                let next = local.len() as u32;
                local.entry(e).or_insert(next);
            }
        }
    }
    let n = local.len();
    let elems: Vec<Vec<(usize, u32)>> = faces
        .iter()
        .map(|f| {
            (0..3)
                .filter_map(|k| local.get(&f.edges[k]).map(|&l| (k, l)))
                .collect()
        })
        .collect();
    let ids: Vec<Vec<u32>> = elems
        .iter()
        .map(|e| e.iter().map(|x| x.1).collect())
        .collect();
    let mut b = Csr::pattern(n, ids.iter().map(|e| &e[..]));
    for (f, e) in faces.iter().zip(&elems) {
        let ke: Vec<f64> = e
            .iter()
            .flat_map(|&(a, _)| e.iter().map(move |&(c, _)| f.mass[a * 3 + c]))
            .collect();
        let nodes: Vec<u32> = e.iter().map(|x| x.1).collect();
        b.add_element(&nodes, &ke);
    }
    let mut c = vec![0.0; n];
    for &(e, v) in &proj {
        if let Some(&l) = local.get(&e) {
            c[l as usize] = v;
        }
    }
    let rhs = c.clone();
    // The edges have no coordinates here; the order of the edges is good enough for 1 face.
    let order: Vec<usize> = (0..n).collect();
    let sym = Symbolic::analyse(&b, &order);
    let chol = Cholesky::factor(&sym, &b)
        .map_err(|e| format!("port {}: the face matrix of the port: {e}", p + 1))?;
    chol.solve(&mut c);
    let norm2: f64 = c.iter().zip(&rhs).map(|(x, y)| x * y).sum();
    if !(norm2 > 0.0) {
        return Err(format!(
            "port {}: the mode is 0 on the free edges of the port",
            p + 1
        ));
    }
    let scale = 1.0 / norm2.sqrt();
    Ok(proj.into_iter().map(|(e, v)| (e, v * scale)).collect())
}

fn empty_set(name: &str) -> String {
    format!(
        "face set {name} selects no face of the model: a face of the model has no part beyond it"
    )
}

type EdgeOf<'a> = dyn Fn(u32, u32) -> u32 + 'a;
type MaterialOf<'a> = dyn Fn(&prism::BoundaryFace) -> &'a Material + 'a;

/// The TE10 mode of a rectangular port face: e = sqrt(2 / (a b)) sin(pi u / a) along the short
/// side, u across the long side a. The 4 sides are electric walls: `magnetic` holds the edges of
/// the magnetic walls.
fn te10(
    p: usize,
    faces: &[&prism::BoundaryFace],
    points: &[[f64; 3]],
    edge: &EdgeOf,
    material: &MaterialOf,
    magnetic: &HashSet<u32>,
) -> Result<(PortMode, Vec<(u32, f64)>), String> {
    let name = p + 1;
    if faces.iter().any(|f| {
        face_pairs(f.nodes)
            .iter()
            .any(|&[a, b]| magnetic.contains(&edge(f.nodes[a], f.nodes[b])))
    }) {
        return Err(format!(
            "port {name}: a magnetic wall cuts the face of the TE10 port, but its mode needs \
             electric walls on all 4 sides: model the full width of the waveguide"
        ));
    }
    let tri = |f: &prism::BoundaryFace| f.nodes.map(|i| points[i as usize]);
    // The normal axis and the plane.
    let n0 = {
        let p = tri(faces[0]);
        let n = cross(
            std::array::from_fn(|d| p[1][d] - p[0][d]),
            std::array::from_fn(|d| p[2][d] - p[0][d]),
        );
        let l = dot3(n, n).sqrt();
        n.map(|v| v / l)
    };
    let axis = (0..3)
        .max_by(|&a, &b| n0[a].abs().total_cmp(&n0[b].abs()))
        .expect("3 axes");
    let plane = points[faces[0].nodes[0] as usize][axis];
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut area = 0.0;
    for f in faces {
        for v in tri(f) {
            for d in 0..3 {
                lo[d] = lo[d].min(v[d]);
                hi[d] = hi[d].max(v[d]);
            }
        }
        area += face_gradients(tri(f)).1;
    }
    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
    let (wu, wv) = (hi[u] - lo[u], hi[v] - lo[v]);
    let flat = (hi[axis] - lo[axis]) <= 1e-9 * wu.max(wv) && (plane - lo[axis]).abs() <= 1e-9 * wu;
    if !flat || (area - wu * wv).abs() > 1e-6 * wu * wv {
        return Err(format!(
            "port {name}: a TE10 port needs a flat rectangular face across x, y or z, with no \
             hole; use a TEM port for a line with a signal conductor"
        ));
    }
    let m0 = material(faces[0]);
    if faces.iter().any(|f| {
        let m = material(f);
        m.permittivity != m0.permittivity || m.permeability != m0.permeability
    }) {
        return Err(format!(
            "port {name}: a TE10 port needs 1 material across its face"
        ));
    }
    // The long side a across the axis `along`, the field along the axis `field`.
    let (along, field, a, b) = if wu >= wv {
        (u, v, wu, wv)
    } else {
        (v, u, wv, wu)
    };
    let amp = (2.0 / (a * b)).sqrt();
    let rule = triangle_rule();
    let mut proj: HashMap<u32, f64> = HashMap::new();
    for f in faces {
        let pts = tri(f);
        let (g, area) = face_gradients(pts);
        let pairs = face_pairs(f.nodes);
        for &[i, j] in &pairs {
            let mut s = 0.0;
            for (l, w) in &rule {
                let x: f64 = (0..3).map(|k| l[k] * pts[k][along]).sum();
                let e = amp * (std::f64::consts::PI * (x - lo[along]) / a).sin();
                s += w * e * (l[i] * g[j][field] - l[j] * g[i][field]);
            }
            *proj.entry(edge(f.nodes[i], f.nodes[j])).or_default() += s * area;
        }
    }
    let mut proj: Vec<(u32, f64)> = proj.into_iter().collect();
    proj.sort_unstable_by_key(|x| x.0);
    Ok((
        PortMode::Te10 {
            kc: std::f64::consts::PI / a,
            eps: m0.permittivity,
            mu: m0.permeability,
        },
        proj,
    ))
}

/// The quasi-TEM mode of a port face from 2 electrostatic solves on its triangles: the points
/// on the signal faces at 1 V, the points on the other electric walls at 0 V.
fn tem(
    p: usize,
    faces: &[&prism::BoundaryFace],
    points: &[[f64; 3]],
    edge: &EdgeOf,
    material: &MaterialOf,
    signal: &[u32],
    ground: &[u32],
) -> Result<(PortMode, Vec<(u32, f64)>), String> {
    let name = p + 1;
    if faces.iter().any(|f| material(f).permeability != 1.0) {
        return Err(format!(
            "port {name}: a TEM port needs a permeability of 1 across its face"
        ));
    }
    // The points of the port, numbered from 0.
    let mut local: HashMap<u32, u32> = HashMap::new();
    let mut global: Vec<u32> = Vec::new();
    for f in faces {
        for v in f.nodes {
            local.entry(v).or_insert_with(|| {
                global.push(v);
                global.len() as u32 - 1
            });
        }
    }
    let n = global.len();
    let mut volt: Vec<Option<f64>> = vec![None; n];
    for &v in ground {
        if let Some(&l) = local.get(&v) {
            volt[l as usize] = Some(0.0);
        }
    }
    let mut touch = false;
    let mut any_signal = false;
    for &v in signal {
        if let Some(&l) = local.get(&v) {
            any_signal = true;
            // A point of the signal on a grounded wall shorts the line.
            touch |= volt[l as usize] == Some(0.0);
            volt[l as usize] = Some(1.0);
        }
    }
    if !any_signal {
        return Err(format!(
            "port {name}: the signal face set has no point on the face of the port"
        ));
    }
    if touch || !volt.contains(&Some(0.0)) {
        return Err(format!(
            "port {name}: a TEM port needs the signal conductor and a ground (an electric wall) \
             on its face, apart from each other"
        ));
    }
    let tris: Vec<[u32; 3]> = faces.iter().map(|f| f.nodes.map(|v| local[&v])).collect();
    let geo: Vec<([[f64; 3]; 3], f64)> = faces
        .iter()
        .map(|f| face_gradients(f.nodes.map(|i| points[i as usize])))
        .collect();
    let eps: Vec<f64> = faces.iter().map(|f| material(f).permittivity).collect();
    let free: Vec<usize> = (0..n).filter(|&i| volt[i].is_none()).collect();
    let mut index = vec![u32::MAX; n];
    for (k, &i) in free.iter().enumerate() {
        index[i] = k as u32;
    }
    let solve = |coef: &dyn Fn(usize) -> f64| -> Result<(Vec<f64>, f64), String> {
        let mut phi: Vec<f64> = volt.iter().map(|v| v.unwrap_or(0.0)).collect();
        if !free.is_empty() {
            let elems: Vec<Vec<u32>> = tris
                .iter()
                .map(|t| {
                    t.iter()
                        .map(|&v| index[v as usize])
                        .filter(|&k| k != u32::MAX)
                        .collect()
                })
                .collect();
            let mut a = Csr::pattern(free.len(), elems.iter().map(|e| &e[..]));
            let mut rhs = vec![0.0; free.len()];
            for (k, t) in tris.iter().enumerate() {
                let (g, area) = geo[k];
                for i in 0..3 {
                    let fi = index[t[i] as usize];
                    if fi == u32::MAX {
                        continue;
                    }
                    for j in 0..3 {
                        let kij = coef(k) * area * dot3(g[i], g[j]);
                        let fj = index[t[j] as usize];
                        if fj == u32::MAX {
                            rhs[fi as usize] -= kij * phi[t[j] as usize];
                        } else {
                            let at = a.find(fi as usize, fj as usize).expect("in the pattern");
                            a.val[at] += kij;
                        }
                    }
                }
            }
            let xyz: Vec<[f64; 3]> = free.iter().map(|&i| points[global[i] as usize]).collect();
            let sym = Symbolic::analyse(&a, &nested_dissection(&a, &xyz));
            let chol = Cholesky::factor(&sym, &a).map_err(|e| {
                format!(
                    "port {name}: the electrostatic solve of the port failed ({e}). Each region \
                     of the face of a TEM port must touch the signal or the ground"
                )
            })?;
            chol.solve(&mut rhs);
            for (k, &i) in free.iter().enumerate() {
                phi[i] = rhs[k];
            }
        }
        // The energy phi^T K phi: the capacitance per length over eps0.
        let mut cap = 0.0;
        for (k, t) in tris.iter().enumerate() {
            let (g, area) = geo[k];
            let grad: [f64; 3] =
                std::array::from_fn(|d| (0..3).map(|i| g[i][d] * phi[t[i] as usize]).sum());
            cap += coef(k) * area * dot3(grad, grad);
        }
        Ok((phi, cap))
    };
    let (phi, cap) = solve(&|k| eps[k])?;
    let (_, cap0) = solve(&|_| 1.0)?;
    if !(cap > 0.0 && cap0 > 0.0) {
        return Err(format!(
            "port {name}: the line has no capacitance between the signal and the ground"
        ));
    }
    // e = -grad phi with a norm of 1: the integral of |grad phi|^2 over the port.
    let fields: Vec<[f64; 3]> = tris
        .iter()
        .zip(&geo)
        .map(|(t, (g, _))| {
            std::array::from_fn(|d| -(0..3).map(|i| g[i][d] * phi[t[i] as usize]).sum::<f64>())
        })
        .collect();
    let norm = fields
        .iter()
        .zip(&geo)
        .map(|(e, (_, area))| area * dot3(*e, *e))
        .sum::<f64>()
        .sqrt();
    let mut proj: HashMap<u32, f64> = HashMap::new();
    for (k, f) in faces.iter().enumerate() {
        let (g, area) = geo[k];
        let e = fields[k].map(|v| v / norm);
        for [i, j] in face_pairs(f.nodes) {
            // The integral of W_ij over the triangle: A / 3 (g_j - g_i).
            let w: [f64; 3] = std::array::from_fn(|d| area / 3.0 * (g[j][d] - g[i][d]));
            *proj.entry(edge(f.nodes[i], f.nodes[j])).or_default() += dot3(w, e);
        }
    }
    let mut proj: Vec<(u32, f64)> = proj.into_iter().collect();
    proj.sort_unstable_by_key(|x| x.0);
    Ok((
        PortMode::Tem {
            eps_eff: cap / cap0,
            cap,
        },
        proj,
    ))
}

/// The diagonal tensors eps and nu = mu^-1 of tetrahedron `e` at the angular frequency `omega`.
fn media(model: &Model, d: &Discrete, part: u32, e: usize, omega: f64) -> ([C64; 3], [C64; 3]) {
    let m = &model.materials[model.parts[part as usize].material];
    let eps = C64::new(m.permittivity, -m.permittivity * m.loss_tangent)
        - C64::J * (m.conductivity / (omega * EPS0));
    let mu = C64::from(m.permeability);
    let Some(stretch) = d.stretch[e] else {
        return ([eps; 3], [C64::ONE / mu; 3]);
    };
    let k = omega / LIGHT * (m.permittivity * m.permeability).sqrt();
    let c: [f64; 3] = std::array::from_fn(|a| {
        d.tets[e]
            .iter()
            .map(|&v| d.points[v as usize][a])
            .sum::<f64>()
            / 4.0
    });
    let s: [C64; 3] = std::array::from_fn(|a| match stretch[a] {
        None => C64::ONE,
        Some(st) => {
            let r = ((c[a] - st.origin).abs() / st.depth).clamp(0.0, 1.0);
            let alpha = 3.0 * (1.0 / PML_REFLECTION).ln() / (2.0 * k * st.depth);
            C64::new(1.0, -alpha * r * r)
        }
    });
    let lambda = [s[1] * s[2] / s[0], s[0] * s[2] / s[1], s[0] * s[1] / s[2]];
    (lambda.map(|l| eps * l), lambda.map(|l| C64::ONE / (mu * l)))
}

/// The matrices S (curl-curl with the face terms of the conductors and the ports) and M (mass)
/// of the unknowns at the angular frequency `omega`, with the propagation constant and mu_r of
/// each port.
fn assemble(model: &Model, d: &Discrete, omega: f64, ports: &[(PortWave, f64)]) -> (Zcsr, Zcsr) {
    let mut s = Zcsr::zeros(d.pattern.clone());
    let mut m = Zcsr::zeros(d.pattern.clone());
    let mut ids = Vec::with_capacity(6);
    let mut local = Vec::with_capacity(6);
    let mut ks = Vec::with_capacity(36);
    let mut ms = Vec::with_capacity(36);
    for e in 0..d.tets.len() {
        ids.clear();
        local.clear();
        for k in 0..6 {
            let u = d.unknown[d.tet_edges[e][k] as usize];
            if u != u32::MAX {
                ids.push(u);
                local.push(k);
            }
        }
        if ids.is_empty() {
            continue;
        }
        let (eps, nu) = media(model, d, d.mesh.part[e], e, omega);
        let g = d.grads[e];
        let v = d.volume[e];
        let pairs = d.tet_pairs[e];
        let curl: [[f64; 3]; 6] = std::array::from_fn(|k| {
            let [a, b] = pairs[k];
            cross(g[a as usize], g[b as usize]).map(|x| 2.0 * x)
        });
        let mm = |a: u8, b: u8| v * if a == b { 2.0 } else { 1.0 } / 20.0;
        let dot_eps = |a: [f64; 3], b: [f64; 3]| {
            eps[0] * (a[0] * b[0]) + eps[1] * (a[1] * b[1]) + eps[2] * (a[2] * b[2])
        };
        ks.clear();
        ms.clear();
        for &p in &local {
            for &q in &local {
                let (cp, cq) = (curl[p], curl[q]);
                ks.push(
                    (nu[0] * (cp[0] * cq[0]) + nu[1] * (cp[1] * cq[1]) + nu[2] * (cp[2] * cq[2]))
                        * v,
                );
                let [i, j] = pairs[p];
                let [k, l] = pairs[q];
                let (gi, gj, gk, gl) = (g[i as usize], g[j as usize], g[k as usize], g[l as usize]);
                ms.push(
                    dot_eps(gj, gl) * mm(i, k)
                        - dot_eps(gj, gk) * mm(i, l)
                        - dot_eps(gi, gl) * mm(j, k)
                        + dot_eps(gi, gk) * mm(j, l),
                );
            }
        }
        s.add_element(&ids, &ks);
        m.add_element(&ids, &ms);
    }
    let mut add_face = |f: &FaceEdges, c: C64| {
        let mut ids = Vec::with_capacity(3);
        let mut local = Vec::with_capacity(3);
        for (k, &e) in f.edges.iter().enumerate() {
            let u = d.unknown[e as usize];
            if u != u32::MAX {
                ids.push(u);
                local.push(k);
            }
        }
        let ke: Vec<C64> = local
            .iter()
            .flat_map(|&a| local.iter().map(move |&b| (a, b)))
            .map(|(a, b)| c * f.mass[a * 3 + b])
            .collect();
        s.add_element(&ids, &ke);
    };
    for (f, sigma) in &d.conductors {
        // j omega mu0 / Zs, Zs = (1 + j) sqrt(omega mu0 / (2 sigma)).
        let zs = C64::new(1.0, 1.0) * (omega * MU0 / (2.0 * sigma)).sqrt();
        add_face(f, C64::J * (omega * MU0) / zs);
    }
    for (port, (wave, mu)) in d.ports.iter().zip(ports) {
        for f in &port.faces {
            add_face(f, C64::J * (wave.beta / mu));
        }
    }
    (s, m)
}

/// E at the centre of each tetrahedron from the edge values `x` of the unknowns.
fn centre_field(d: &Discrete, x: &[C64]) -> Vec<[C64; 3]> {
    (0..d.tets.len())
        .map(|e| {
            let g = d.grads[e];
            let mut f = [C64::ZERO; 3];
            for k in 0..6 {
                let u = d.unknown[d.tet_edges[e][k] as usize];
                if u == u32::MAX {
                    continue;
                }
                let [a, b] = d.tet_pairs[e][k];
                // W_ab at the centre: (grad l_b - grad l_a) / 4.
                for c in 0..3 {
                    f[c] += x[u as usize] * ((g[b as usize][c] - g[a as usize][c]) / 4.0);
                }
            }
            f
        })
        .collect()
}

fn sweep(
    model: &Model,
    d: &Discrete,
    start: f64,
    stop: f64,
    points: usize,
    output_every: usize,
    events: &mut dyn Events,
) -> Result<(), String> {
    let np = d.ports.len();
    for k in 0..points {
        if events.cancelled() {
            return Err("cancelled".into());
        }
        let f = if points == 1 {
            start
        } else {
            start + (stop - start) * k as f64 / (points - 1) as f64
        };
        let omega = 2.0 * std::f64::consts::PI * f;
        let k0 = omega / LIGHT;
        let waves: Vec<(PortWave, f64)> = d
            .ports
            .iter()
            .enumerate()
            .map(|(p, port)| port.wave(omega, p))
            .collect::<Result<_, _>>()?;
        let (mut a, m) = assemble(model, d, omega, &waves);
        let shift = C64::from(k0 * k0);
        for (v, w) in a.val.iter_mut().zip(&m.val) {
            *v -= *w * shift;
        }
        let solver = match &d.sym {
            Some(sym) => {
                drop(m);
                Solver::Direct(factor(sym, &a).map_err(|e| {
                    format!(
                        "{e}. A closed lossless model at one of its resonant frequencies is \
                         singular: move the frequency, or add a port or a loss"
                    )
                })?)
            }
            None => {
                // P = S + c k0^2 M = A + (c + 1) k0^2 M.
                let mut p = a.clone();
                for (v, w) in p.val.iter_mut().zip(&m.val) {
                    *v += *w * (shift * (SHIFT + C64::ONE));
                }
                drop(m);
                Solver::Iterative(auxiliary(model, d, omega, p)?)
            }
        };
        let mut iterations = Vec::new();
        let mut s = vec![C64::ZERO; np * np];
        let mut fields = Vec::new();
        let with_fields = k % output_every == 0 || k + 1 == points;
        for (j, port) in d.ports.iter().enumerate() {
            let (wave, mu) = waves[j];
            let amp = (2.0 * wave.impedance_wave(omega, mu)).sqrt();
            let mut x = vec![C64::ZERO; d.unknowns];
            let c = C64::J * (2.0 * wave.beta / mu * amp);
            for &(e, p) in &port.proj {
                let u = d.unknown[e as usize];
                if u != u32::MAX {
                    x[u as usize] += c * p;
                }
            }
            match &solver {
                Solver::Direct(factor) => factor.solve(&mut x),
                Solver::Iterative(aux) => {
                    let b = std::mem::replace(&mut x, vec![C64::ZERO; d.unknowns]);
                    let n = hx::gmres(&a, &b, &mut x, &|r, z| aux.apply(r, z), GMRES, &mut || {
                        events.cancelled()
                    })?;
                    iterations.push(n.to_string());
                }
            }
            for (i, other) in d.ports.iter().enumerate() {
                let mut b = C64::ZERO;
                for &(e, p) in &other.proj {
                    let u = d.unknown[e as usize];
                    if u != u32::MAX {
                        b += x[u as usize] * p;
                    }
                }
                if i == j {
                    b -= C64::from(amp);
                }
                let (wi, mui) = waves[i];
                let zi = wi.impedance_wave(omega, mui);
                let zj = wave.impedance_wave(omega, mu);
                let shift = C64::cis(wi.beta * other.deembed + wave.beta * port.deembed);
                s[i * np + j] = b / zi.sqrt() / (amp / zj.sqrt()) * shift;
            }
            if with_fields {
                fields.push(centre_field(d, &x));
            }
        }
        if !iterations.is_empty() {
            events.progress(&format!(
                "{f:.6e} Hz: GMRES iterations {}",
                iterations.join(", ")
            ));
        }
        events.frame(&Frame {
            frequency: f,
            fields,
            s,
            ports: waves.iter().map(|w| w.0).collect(),
            q: None,
        })?;
    }
    Ok(())
}

/// The settings of GMRES. Reason for the restart: each step stores 1 vector of the unknowns, so 50
/// steps of 1,000,000 unknowns use 0.8 GB.
const GMRES: hx::Settings = hx::Settings {
    restart: 50,
    tolerance: 1e-8,
    iterations: 3000,
};
const SHIFT: C64 = C64::new(SHIFT_RE, SHIFT_IM);
const SHIFT_RE: f64 = -1.0;
const SHIFT_IM: f64 = 0.5;

/// The complex symmetric factor of `a`, with its real and its imaginary part as 2 real matrices
/// of the pattern of `a`.
fn factor(sym: &Symbolic, a: &Zcsr) -> Result<Ldlt, String> {
    let part = |f: fn(&C64) -> f64| Csr {
        val: a.val.iter().map(f).collect(),
        ..a.pattern.clone()
    };
    Ldlt::factor(sym, &part(|v| v.re), &part(|v| v.im))
}

#[allow(clippy::large_enum_variant)]
enum Solver {
    Direct(Ldlt),
    Iterative(hx::Auxiliary),
}

/// The preconditioner of the iterative solver for the shifted operator `p` at `omega`. The vector
/// space has the points that are not on an electric wall (no fixed edge touches them). The
/// gradient space has the same points, and 1 potential for each connected electric wall but the
/// largest (the ground): a line has the field of the potential of its signal conductor. Reason:
/// with no potential on the conductors, the iterative solve of the coaxial line of `tests/em.rs`
/// took 2676 GMRES iterations.
fn auxiliary(model: &Model, d: &Discrete, omega: f64, p: Zcsr) -> Result<hx::Auxiliary, String> {
    let k2 = SHIFT * (omega / LIGHT).powi(2);
    let n = d.points.len();
    let mut fixed = vec![false; n];
    // The connected electric walls: a union of the points of the fixed edges.
    let mut root: Vec<u32> = (0..n as u32).collect();
    fn find(root: &mut [u32], mut a: u32) -> u32 {
        while root[a as usize] != a {
            root[a as usize] = root[root[a as usize] as usize];
            a = root[a as usize];
        }
        a
    }
    for (e, &[a, b]) in d.edges.iter().enumerate() {
        if d.unknown[e] == u32::MAX {
            fixed[a as usize] = true;
            fixed[b as usize] = true;
            let (ra, rb) = (find(&mut root, a), find(&mut root, b));
            root[ra as usize] = rb;
        }
    }
    let mut vindex = vec![u32::MAX; n];
    let mut xyz = Vec::new();
    for tet in &d.tets {
        for &v in tet {
            if !fixed[v as usize] && vindex[v as usize] == u32::MAX {
                vindex[v as usize] = xyz.len() as u32;
                xyz.push(d.points[v as usize]);
            }
        }
    }
    let free = xyz.len();
    let mut size: HashMap<u32, usize> = HashMap::new();
    for v in 0..n as u32 {
        if fixed[v as usize] {
            *size.entry(find(&mut root, v)).or_default() += 1;
        }
    }
    let ground = size
        .iter()
        .max_by_key(|(r, s)| (**s, std::cmp::Reverse(**r)))
        .map(|(r, _)| *r);
    let mut conductors: Vec<u32> = size
        .keys()
        .copied()
        .filter(|r| Some(*r) != ground)
        .collect();
    conductors.sort_unstable();
    let mut gindex = vindex.clone();
    let mut gxyz = xyz.clone();
    for (c, &r) in conductors.iter().enumerate() {
        gxyz.push(d.points[r as usize]);
        for v in 0..n as u32 {
            if fixed[v as usize] && find(&mut root, v) == r {
                gindex[v as usize] = (free + c) as u32;
            }
        }
    }
    // The ids of each tetrahedron in a space, with the local points of each id.
    let ids = |index: &[u32], tet: &[u32; 4]| -> Vec<(u32, Vec<usize>)> {
        let mut out: Vec<(u32, Vec<usize>)> = Vec::new();
        for (k, &v) in tet.iter().enumerate() {
            let id = index[v as usize];
            if id == u32::MAX {
                continue;
            }
            match out.iter_mut().find(|(i, _)| *i == id) {
                Some((_, l)) => l.push(k),
                None => out.push((id, vec![k])),
            }
        }
        out
    };
    let vids: Vec<Vec<u32>> = d
        .tets
        .iter()
        .map(|t| ids(&vindex, t).into_iter().map(|(i, _)| i).collect())
        .collect();
    let gids: Vec<Vec<u32>> = d
        .tets
        .iter()
        .map(|t| ids(&gindex, t).into_iter().map(|(i, _)| i).collect())
        .collect();
    let mut vector = Zcsr::zeros(Csr::pattern(free, vids.iter().map(|e| &e[..])));
    let mut gradient = Zcsr::zeros(Csr::pattern(gxyz.len(), gids.iter().map(|e| &e[..])));
    for (e, tet) in d.tets.iter().enumerate() {
        let (eps, nu) = media(model, d, d.mesh.part[e], e, omega);
        let (eps_mean, nu_mean) = (
            (eps[0] + eps[1] + eps[2]) * (1.0 / 3.0),
            (nu[0] + nu[1] + nu[2]) * (1.0 / 3.0),
        );
        let g = d.grads[e];
        let v = d.volume[e];
        let kv = |a: usize, b: usize| {
            let gg = g[a][0] * g[b][0] + g[a][1] * g[b][1] + g[a][2] * g[b][2];
            let mass = v * if a == b { 2.0 } else { 1.0 } / 20.0;
            nu_mean * (v * gg) + eps_mean * k2 * mass
        };
        let kg = |a: usize, b: usize| {
            (eps[0] * (g[a][0] * g[b][0])
                + eps[1] * (g[a][1] * g[b][1])
                + eps[2] * (g[a][2] * g[b][2]))
                * k2
                * v
        };
        for (matrix, index, local) in [
            (&mut vector, &vindex, &kv as &dyn Fn(usize, usize) -> C64),
            (&mut gradient, &gindex, &kg),
        ] {
            let groups = ids(index, tet);
            let ke: Vec<C64> = groups
                .iter()
                .flat_map(|(_, la)| groups.iter().map(move |(_, lb)| (la, lb)))
                .map(|(la, lb)| {
                    la.iter()
                        .flat_map(|&a| lb.iter().map(move |&b| (a, b)))
                        .fold(C64::ZERO, |s, (a, b)| s + local(a, b))
                })
                .collect();
            let id: Vec<u32> = groups.iter().map(|(i, _)| *i).collect();
            matrix.add_element(&id, &ke);
        }
    }
    let vsym = Symbolic::analyse(&vector.pattern, &nested_dissection(&vector.pattern, &xyz));
    let vector = factor(&vsym, &vector)?;
    drop(vsym);
    let gsym = Symbolic::analyse(
        &gradient.pattern,
        &nested_dissection(&gradient.pattern, &gxyz),
    );
    let gradient = factor(&gsym, &gradient)?;
    let edges = d
        .edges
        .iter()
        .enumerate()
        .filter(|(e, _)| d.unknown[*e] != u32::MAX)
        .map(|(e, &[a, b])| {
            let (pa, pb) = (d.points[a as usize], d.points[b as usize]);
            hx::Edge {
                unknown: d.unknown[e],
                vector: [vindex[a as usize], vindex[b as usize]],
                gradient: [gindex[a as usize], gindex[b as usize]],
                half: std::array::from_fn(|c| 0.5 * (pb[c] - pa[c])),
            }
        })
        .collect();
    Ok(hx::Auxiliary::new(
        p,
        edges,
        vector,
        gradient,
        free,
        gxyz.len(),
    ))
}

impl PortWave {
    /// The wave impedance omega mu / beta of the mode.
    fn impedance_wave(&self, omega: f64, mu: f64) -> f64 {
        omega * MU0 * mu / self.beta
    }
}

/// The resonant modes nearest to `near`.
fn modes(
    model: &Model,
    d: &Discrete,
    near: f64,
    count: usize,
    events: &mut dyn Events,
) -> Result<(), String> {
    let (mut found, iterations) = eigen(model, d, near, count, events)?;
    events.progress(&format!(
        "{count} modes converged in {iterations} iterations"
    ));
    // The face terms of good conductors, the conductivity of a material and the absorbing layers
    // change with the frequency. Each mode solves again at its own frequency until its frequency
    // stops changing: the face terms are then those of the mode. Reason: with the terms at the
    // target 8 GHz, the Q of the TE101 mode at 8.89 GHz of a copper cavity was 5 % low
    // (validation/cases/grid-cavity-te101).
    let dispersive = !d.conductors.is_empty()
        || model.pml.iter().any(|&p| p)
        || model.materials.iter().any(|m| m.conductivity > 0.0);
    if dispersive {
        // Each mode keeps the new mode nearest to its own vector, so 2 modes with near
        // frequencies stay 2 modes.
        let total = found.len();
        for (theta, v) in found.iter_mut() {
            for _ in 0..5 {
                let f = frequency_of(*theta);
                let (again, _) = eigen(model, d, f, total, events)?;
                let (_, m) = assemble(model, d, 2.0 * std::f64::consts::PI * f, &[]);
                let mut mv = vec![C64::ZERO; v.len()];
                m.mul(v, &mut mv);
                let overlap = |w: &[C64]| {
                    complex::dot(w, &mv).abs() / complex::norm(w).max(f64::MIN_POSITIVE)
                };
                let (t, w) = again
                    .into_iter()
                    .max_by(|a, b| overlap(&a.1).total_cmp(&overlap(&b.1)))
                    .expect("a mode solve finds 1 mode or more");
                *theta = t;
                *v = w;
                if (frequency_of(t) - f).abs() < 1e-9 * f {
                    break;
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.re.total_cmp(&b.0.re));
    for (theta, v) in found {
        let k = theta.sqrt();
        let f = frequency_of(theta);
        let q = if k.im.abs() > 0.0 {
            k.re / (2.0 * k.im.abs())
        } else {
            f64::INFINITY
        };
        let mut field = centre_field(d, &v);
        let peak = field
            .iter()
            .map(|e| e.iter().map(|c| c.norm2()).sum::<f64>().sqrt())
            .fold(0.0, f64::max);
        // The phase of the largest component is 0.
        let top = field
            .iter()
            .flatten()
            .max_by(|a, b| a.norm2().total_cmp(&b.norm2()))
            .copied()
            .unwrap_or(C64::ONE);
        let scale = if top.abs() > 0.0 {
            top.conj() / (top.abs() * peak)
        } else {
            C64::ONE
        };
        for e in field.iter_mut() {
            for c in e.iter_mut() {
                *c *= scale;
            }
        }
        events.frame(&Frame {
            frequency: f,
            fields: vec![field],
            s: Vec::new(),
            ports: Vec::new(),
            q: Some(q),
        })?;
    }
    Ok(())
}

/// The frequency (Hz) of the eigenvalue k^2.
fn frequency_of(theta: C64) -> f64 {
    LIGHT * theta.sqrt().re / (2.0 * std::f64::consts::PI)
}

/// The `count` modes nearest the frequency `near`, with the face terms at `near`, and the number
/// of iterations.
#[allow(clippy::type_complexity)]
fn eigen(
    model: &Model,
    d: &Discrete,
    near: f64,
    count: usize,
    events: &mut dyn Events,
) -> Result<(Vec<(C64, Vec<C64>)>, usize), String> {
    let omega = 2.0 * std::f64::consts::PI * near;
    let sigma = (omega / LIGHT).powi(2);
    let (s, m) = assemble(model, d, omega, &[]);
    let mut a = s.clone();
    for (v, w) in a.val.iter_mut().zip(&m.val) {
        *v -= *w * sigma;
    }
    let sym = d.sym.as_ref().expect("a mode solve has the direct solver");
    let factor = factor(sym, &a)?;
    drop(a);
    let gauge = Gauge::new(model, d, omega)?;
    let n = d.unknowns;
    let p = (2 * count + 4).min(n);
    // Start vectors with no symmetry.
    let mut x: Vec<Vec<C64>> = (0..p)
        .map(|j| {
            (0..n)
                .map(|i| {
                    let t = (i * (j + 1)) as f64;
                    C64::new((0.37 * t + j as f64).sin(), (0.11 * t).cos() * 0.5)
                })
                .collect()
        })
        .collect();
    let mut y = vec![C64::ZERO; n];
    let mut found: Vec<(C64, Vec<C64>)> = Vec::new();
    for iteration in 0..300 {
        if events.cancelled() {
            return Err("cancelled".into());
        }
        for v in x.iter_mut() {
            m.mul(v, &mut y);
            factor.solve(&mut y);
            std::mem::swap(v, &mut y);
            if let Some(g) = &gauge {
                g.project(&m, v);
            }
        }
        orthonormalize(&mut x);
        // Rayleigh-Ritz with the bilinear forms of the complex symmetric S and M.
        let sx: Vec<Vec<C64>> = x
            .iter()
            .map(|v| {
                let mut w = vec![C64::ZERO; n];
                s.mul(v, &mut w);
                w
            })
            .collect();
        let mx: Vec<Vec<C64>> = x
            .iter()
            .map(|v| {
                let mut w = vec![C64::ZERO; n];
                m.mul(v, &mut w);
                w
            })
            .collect();
        let q = x.len();
        let mut kr = Matrix::zeros(q);
        let mut mr = Matrix::zeros(q);
        for i in 0..q {
            for j in 0..q {
                kr.set(i, j, complex::dot(&x[i], &sx[j]));
                mr.set(i, j, complex::dot(&x[i], &mx[j]));
            }
        }
        // H = Mr^-1 Kr.
        let Ok(h) = mr.solve(&kr) else {
            return Err("the subspace of the mode solve lost its rank".into());
        };
        let theta = h.eigenvalues();
        // The physical modes first, nearest to the target; a value near 0 is a gradient field.
        let mut order: Vec<usize> = (0..q).collect();
        let spurious = |t: C64| t.abs() < 1e-6 * sigma;
        order.sort_by(|&i, &j| {
            (spurious(theta[i]), (theta[i] - C64::from(sigma)).abs())
                .partial_cmp(&(spurious(theta[j]), (theta[j] - C64::from(sigma)).abs()))
                .expect("finite")
        });
        let mut next = Vec::with_capacity(q);
        let mut done = true;
        found.clear();
        for (rank, &k) in order.iter().enumerate() {
            let yk = h.eigenvector(theta[k]);
            let mut v = vec![C64::ZERO; n];
            let mut sv = vec![C64::ZERO; n];
            let mut mv = vec![C64::ZERO; n];
            for (c, coef) in yk.iter().enumerate() {
                for i in 0..n {
                    v[i] += x[c][i] * *coef;
                    sv[i] += sx[c][i] * *coef;
                    mv[i] += mx[c][i] * *coef;
                }
            }
            if rank < count {
                let r: Vec<C64> = sv
                    .iter()
                    .zip(&mv)
                    .map(|(a, b)| *a - *b * theta[k])
                    .collect();
                let rel = complex::norm(&r) / complex::norm(&sv).max(f64::MIN_POSITIVE);
                if spurious(theta[k]) || !(rel < 1e-8) {
                    done = false;
                }
                found.push((theta[k], v.clone()));
            }
            next.push(v);
        }
        x = next;
        orthonormalize(&mut x);
        if done {
            return Ok((found, iteration + 1));
        }
        if iteration == 299 {
            return Err(format!(
                "the mode solve did not converge in 300 iterations: give a near closer to the \
                 modes (near = {near:.4e} Hz)"
            ));
        }
    }
    unreachable!("the loop returns")
}

/// The projection P x = x - G (G^T M G)^-1 G^T M x that removes the gradient fields G phi
/// from the field x, phi on the points with no electric or conductor wall. The gradients of
/// these points are the null space of the curl with the face terms, with the eigenvalue 0; a
/// mode with an eigenvalue other than 0 has G^T M x = 0. Shift and invert alone leaves them in
/// the subspace when the target is nearer to 0 than to a mode. G^T M G is the matrix of eps
/// grad phi_a . grad phi_b of the linear functions of the points (Bossavit 1998, chapter 5).
struct Gauge {
    /// The edges of the unknowns with the index of their 2 points, or `u32::MAX`.
    edges: Vec<(u32, u32, u32)>,
    points: usize,
    factor: Ldlt,
}

impl Gauge {
    fn new(model: &Model, d: &Discrete, omega: f64) -> Result<Option<Gauge>, String> {
        let mut index = vec![u32::MAX; d.points.len()];
        let mut xyz = Vec::new();
        for tet in &d.tets {
            for &v in tet {
                if !d.walled[v as usize] && index[v as usize] == u32::MAX {
                    index[v as usize] = xyz.len() as u32;
                    xyz.push(d.points[v as usize]);
                }
            }
        }
        if xyz.is_empty() {
            return Ok(None);
        }
        let ids: Vec<Vec<u32>> = d
            .tets
            .iter()
            .map(|t| {
                t.iter()
                    .map(|&v| index[v as usize])
                    .filter(|&k| k != u32::MAX)
                    .collect()
            })
            .collect();
        let mut l = Zcsr::zeros(Csr::pattern(xyz.len(), ids.iter().map(|e| &e[..])));
        for (e, tet) in d.tets.iter().enumerate() {
            let (eps, _) = media(model, d, d.mesh.part[e], e, omega);
            let g = d.grads[e];
            let local: Vec<usize> = (0..4)
                .filter(|&k| index[tet[k] as usize] != u32::MAX)
                .collect();
            let ke: Vec<C64> = local
                .iter()
                .flat_map(|&a| local.iter().map(move |&b| (a, b)))
                .map(|(a, b)| {
                    (eps[0] * (g[a][0] * g[b][0])
                        + eps[1] * (g[a][1] * g[b][1])
                        + eps[2] * (g[a][2] * g[b][2]))
                        * d.volume[e]
                })
                .collect();
            l.add_element(&ids[e], &ke);
        }
        let sym = Symbolic::analyse(&l.pattern, &nested_dissection(&l.pattern, &xyz));
        let factor = factor(&sym, &l)?;
        let edges = d
            .edges
            .iter()
            .enumerate()
            .filter(|(e, _)| d.unknown[*e] != u32::MAX)
            .map(|(e, &[a, b])| (d.unknown[e], index[a as usize], index[b as usize]))
            .collect();
        Ok(Some(Gauge {
            edges,
            points: xyz.len(),
            factor,
        }))
    }

    fn project(&self, m: &Zcsr, x: &mut [C64]) {
        let mut y = vec![C64::ZERO; x.len()];
        m.mul(x, &mut y);
        let mut phi = vec![C64::ZERO; self.points];
        for &(u, a, b) in &self.edges {
            let v = y[u as usize];
            if b != u32::MAX {
                phi[b as usize] += v;
            }
            if a != u32::MAX {
                phi[a as usize] -= v;
            }
        }
        self.factor.solve(&mut phi);
        let at = |k: u32| {
            if k == u32::MAX {
                C64::ZERO
            } else {
                phi[k as usize]
            }
        };
        for &(u, a, b) in &self.edges {
            x[u as usize] -= at(b) - at(a);
        }
    }
}

/// Modified Gram-Schmidt with the Hermitian inner product; a vector that is dependent on the
/// ones before it gets a new direction.
fn orthonormalize(x: &mut [Vec<C64>]) {
    for i in 0..x.len() {
        let (done, rest) = x.split_at_mut(i);
        let v = &mut rest[0];
        for u in done.iter() {
            let c = u
                .iter()
                .zip(v.iter())
                .fold(C64::ZERO, |s, (a, b)| s + a.conj() * *b);
            for (vi, ui) in v.iter_mut().zip(u) {
                *vi -= *ui * c;
            }
        }
        let n = complex::norm(v);
        if n > 0.0 {
            for vi in v.iter_mut() {
                *vi = *vi / n;
            }
        }
    }
}
