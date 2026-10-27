//! Steady and transient heat conduction on the box grid, with 8-node hex elements.
//!
//! The model is linear: each material has a constant conductivity (1 value, or 1 value for each
//! axis), density and specific heat. The loads are a fixed temperature, a heat flux into the
//! body and a convection on named faces, and a heat source in the volume of a part. Parts that
//! touch share their nodes, so the heat crosses with no contact resistance. A transient step is
//! backward Euler with a lumped capacity, so a large step gives no oscillation.
//!
//! The element matrices of a box cell are exact tensor products of the 1D matrices of the
//! linear element: the conductivity k/h [1 -1; -1 1] and the mass h/6 [2 1; 1 2].
//!
//! Each result has an energy balance: the heat from the volume sources, the fluxes, the
//! convection and the fixed temperatures, and the rate of the stored heat. Their sum is 0 to the
//! precision of the linear solve.
//!
//! Sources: T. J. R. Hughes, The Finite Element Method: Linear Static and Dynamic Finite Element
//! Analysis, Prentice-Hall 1987, chapters 1 to 3 (the heat equation, the hex element) and 8
//! (the generalized trapezoidal family, backward Euler, lumped capacity); F. P. Incropera, D. P.
//! DeWitt, T. L. Bergman and A. S. Lavine, Fundamentals of Heat and Mass Transfer, 6th ed.,
//! Wiley 2007, chapters 2 and 3 (the conduction equation, convection and the boundary
//! conditions).

use crate::grid::{tol, Face, Grid, Part, Side, Spacing, VOID};
use crate::multigrid::Level;
use crate::solve::{Method, Solver};
use crate::sparse::Csr;

/// A material. `conductivity` is the value along x, y and z.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub conductivity: [f64; 3],
    /// Density times specific heat. A transient step needs it above 0 in each part.
    pub capacity: f64,
}

/// A named selection of the faces of the model.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceSet {
    pub name: String,
    /// The sides of the cells (empty: all 6).
    pub sides: Vec<Side>,
    /// The faces with the centre in this box (`None`: all).
    pub region: Option<([f64; 3], [f64; 3])>,
    /// The faces of the cells of these parts (empty: all parts).
    pub parts: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Load {
    /// A fixed temperature on the faces of a face set.
    Temperature { faces: usize, value: f64 },
    /// A heat flux into the body (power per area) on the faces of a face set.
    Flux { faces: usize, value: f64 },
    /// A convection to a fluid at the temperature `sink` with the film coefficient `film`.
    Convection { faces: usize, film: f64, sink: f64 },
    /// A heat source (power per volume) in a part.
    Source { part: usize, value: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Steady,
    /// Backward Euler from the temperature `initial` at time 0 to `end`, in steps of `dt`. The
    /// result has the time 0 and every `output_every`-th step, and the last step.
    Transient {
        end: f64,
        dt: f64,
        initial: f64,
        output_every: usize,
    },
}

/// A point where the result gives the temperature.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub point: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub spacing: Spacing,
    /// Solve the half above the middle plane of the parts of each axis that is true. The parts,
    /// the face sets of the loads and the loads must mirror at that plane.
    pub symmetry: [bool; 3],
    pub faces: Vec<FaceSet>,
    pub loads: Vec<Load>,
    pub step: Step,
    pub probes: Vec<Probe>,
    /// The linear solver, else the size of the model chooses it.
    pub method: Option<Method>,
}

/// The mesh of the result: the grid points of the cells of the parts, and the hex cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub points: Vec<[f64; 3]>,
    /// The points of each cell, in the order of a VTK hexahedron.
    pub cells: Vec<[u32; 8]>,
    /// The part of each cell.
    pub part: Vec<u32>,
}

/// The heat rates of a result (power). Heat into the body is positive.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Energy {
    pub source: f64,
    pub flux: f64,
    pub convection: f64,
    /// The heat that the fixed temperatures put in.
    pub fixed: f64,
    /// The rate of the stored heat.
    pub stored: f64,
    /// The sum of the magnitudes of the heat flows at each point and each face: the scale of the
    /// imbalance. Heat that goes in at 1 face and out at another counts twice.
    pub gross: f64,
}

impl Energy {
    /// (source + flux + convection + fixed - stored) / gross.
    pub fn imbalance(&self) -> f64 {
        let sum = self.source + self.flux + self.convection + self.fixed - self.stored;
        if self.gross == 0.0 {
            0.0
        } else {
            sum / self.gross
        }
    }
}

/// The result at 1 output time.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub time: f64,
    /// The temperature of each point of the mesh.
    pub temperature: Vec<f64>,
    /// The heat flux at the centre of each cell.
    pub flux: Vec<[f64; 3]>,
    /// The temperature at each probe.
    pub probes: Vec<f64>,
    pub energy: Energy,
}

/// What a solve tells while it runs.
pub trait Events {
    /// True stops the solve.
    fn cancelled(&self) -> bool {
        false
    }
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String>;
    /// A line of progress for the person.
    fn progress(&mut self, text: &str);
    fn frame(&mut self, frame: &Frame) -> Result<(), String>;
}

/// The end of a solve.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub cells: usize,
    pub points: usize,
    pub unknowns: usize,
    pub method: Method,
    /// The largest relative energy imbalance of the output times.
    pub imbalance: f64,
    /// The things that the person must know about the result.
    pub warnings: Vec<String>,
}

/// A grid with more points than this needs more memory than a laptop has. Reason: a steady
/// multigrid solve of 1,009,057 points used 1.9 GB (the peak resident size), so 4,000,000 points
/// need about 7.5 GB, half of the RAM of a 16 GB laptop.
const MAX_POINTS: usize = 4_000_000;

/// An energy imbalance above this fraction is a warning.
const IMBALANCE: f64 = 1e-6;

/// Solve the model.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Summary, String> {
    check(model)?;
    let mut warnings = Vec::new();
    let (grid, faces, mids) = build_grid(model)?;
    if mids.iter().any(Option::is_some) {
        let names: Vec<String> = (0..3)
            .filter_map(|d| mids[d].map(|m| format!("{} = {m}", ["x", "y", "z"][d])))
            .collect();
        warnings.push(format!(
            "the model is the part above the planes of symmetry {}: the energy terms are those of \
             this part",
            names.join(" and ")
        ));
    }
    // The points of the cells of the parts.
    let dims = grid.dims();
    let mut number = vec![u32::MAX; (dims[0] + 1) * (dims[1] + 1) * (dims[2] + 1)];
    let mut ijk: Vec<[u32; 3]> = Vec::new();
    let mut cells: Vec<[u32; 8]> = Vec::new();
    let mut cell_of: Vec<usize> = Vec::new();
    for cell in 0..grid.part.len() {
        if grid.part[cell] == VOID {
            continue;
        }
        let corners = grid.corners(cell);
        let mut nodes = [0u32; 8];
        for (c, p) in corners.iter().enumerate() {
            let slot = &mut number[grid.point_index(*p)];
            if *slot == u32::MAX {
                *slot = ijk.len() as u32;
                ijk.push([p[0] as u32, p[1] as u32, p[2] as u32]);
                if ijk.len() > MAX_POINTS {
                    return Err(format!(
                        "the grid has more than {MAX_POINTS} points, more than the memory of a \
                         laptop solves: use a larger max_cell or a larger max_growth"
                    ));
                }
            }
            nodes[c] = *slot;
        }
        cells.push(nodes);
        cell_of.push(cell);
    }
    let n = ijk.len();
    let points: Vec<[f64; 3]> = ijk
        .iter()
        .map(|p| grid.point([p[0] as usize, p[1] as usize, p[2] as usize]))
        .collect();
    let mesh = Mesh {
        points,
        cells,
        part: cell_of.iter().map(|&c| grid.part[c]).collect(),
    };
    events.mesh(&mesh)?;
    events.progress(&format!(
        "grid {} x {} x {} planes, {} cells, {} points",
        dims[0] + 1,
        dims[1] + 1,
        dims[2] + 1,
        mesh.cells.len(),
        n
    ));
    let node_of = |p: [usize; 3]| number[grid.point_index(p)] as usize;

    // The conductivity matrix with the convection, the lumped capacity and the loads.
    let mut k = Csr::pattern(n, mesh.cells.iter().map(|c| &c[..]));
    let mut capacity = vec![0.0; n];
    let mut source = vec![0.0; n];
    for (e, &cell) in cell_of.iter().enumerate() {
        let m = &model.materials[model.parts[grid.part[cell] as usize].material];
        let size = grid.size(cell);
        k.add_element(&mesh.cells[e], &conduction(size, m.conductivity));
        let volume = size[0] * size[1] * size[2];
        for &node in &mesh.cells[e] {
            capacity[node as usize] += m.capacity * volume / 8.0;
        }
    }
    let mut part_source = vec![0.0; model.parts.len()];
    for load in &model.loads {
        if let Load::Source { part, value } = load {
            part_source[*part] += value;
        }
    }
    for (e, &cell) in cell_of.iter().enumerate() {
        let q = part_source[grid.part[cell] as usize];
        if q != 0.0 {
            let s = grid.size(cell);
            for &node in &mesh.cells[e] {
                source[node as usize] += q * s[0] * s[1] * s[2] / 8.0;
            }
        }
    }
    let mut flux = vec![0.0; n];
    let mut film = vec![0.0; n];
    let mut fixed: Vec<Option<(f64, usize)>> = vec![None; n];
    // The convection faces: the 4 nodes, the area, the film coefficient and the sink.
    let mut convection: Vec<([usize; 4], f64, f64, f64)> = Vec::new();
    for load in &model.loads {
        let set = match load {
            Load::Source { .. } => continue,
            Load::Temperature { faces, .. }
            | Load::Flux { faces, .. }
            | Load::Convection { faces, .. } => *faces,
        };
        for &face in &faces[set] {
            let nodes = grid.face_points(face).map(node_of);
            let area = grid.face_area(face);
            match load {
                Load::Temperature { value, faces: f } => {
                    for &node in &nodes {
                        match fixed[node] {
                            Some((v, other)) if v != *value => {
                                return Err(format!(
                                    "the face sets {} and {} share a point with 2 fixed \
                                     temperatures, {v} and {value}",
                                    model.faces[other].name, model.faces[*f].name
                                ))
                            }
                            _ => fixed[node] = Some((*value, *f)),
                        }
                    }
                }
                Load::Flux { value, .. } => {
                    for &node in &nodes {
                        flux[node] += value * area / 4.0;
                    }
                }
                Load::Convection {
                    film: h, sink: t, ..
                } => {
                    let s = grid.size(face.cell);
                    let side = Side::ALL[face.side as usize];
                    let (a, b) = match side.axis {
                        0 => (s[1], s[2]),
                        1 => (s[0], s[2]),
                        _ => (s[0], s[1]),
                    };
                    let ids = nodes.map(|v| v as u32);
                    k.add_element(&ids, &face_mass(a, b, *h));
                    for &node in &nodes {
                        film[node] += h * t * area / 4.0;
                    }
                    convection.push((nodes, area, *h, *t));
                }
                Load::Source { .. } => unreachable!("skipped above"),
            }
        }
    }
    let transient = matches!(model.step, Step::Transient { .. });
    if !transient {
        let mut held: Vec<bool> = fixed.iter().map(Option::is_some).collect();
        for (nodes, ..) in &convection {
            for &v in nodes {
                held[v] = true;
            }
        }
        check_supports(model, &mesh, &held)?;
    }
    let load: Vec<f64> = (0..n).map(|i| source[i] + flux[i] + film[i]).collect();
    let free: Vec<usize> = (0..n).filter(|&i| fixed[i].is_none()).collect();
    let mut temperature: Vec<f64> = match model.step {
        Step::Steady => vec![0.0; n],
        Step::Transient { initial, .. } => vec![initial; n],
    };
    for i in 0..n {
        if let Some((v, _)) = fixed[i] {
            temperature[i] = v;
        }
    }
    let level = Level {
        planes: grid.planes.clone(),
        points: free.iter().map(|&i| ijk[i]).collect(),
    };
    let energy_of = |t: &[f64], previous: &[f64], dt: Option<f64>, kt: &[f64]| -> Energy {
        let mut e = Energy {
            source: source.iter().sum(),
            flux: flux.iter().sum(),
            gross: source.iter().chain(&flux).map(|v| v.abs()).sum(),
            ..Energy::default()
        };
        for (nodes, area, h, sink) in &convection {
            let mean = nodes.iter().map(|&v| t[v]).sum::<f64>() / 4.0;
            let q = h * area * (sink - mean);
            e.convection += q;
            e.gross += q.abs();
        }
        for i in 0..n {
            let rate = dt.map_or(0.0, |dt| capacity[i] * (t[i] - previous[i]) / dt);
            e.stored += rate;
            e.gross += rate.abs();
            if fixed[i].is_some() {
                // The heat that the fixed point takes from the body, minus the loads on it.
                let q = kt[i] + rate - load[i];
                e.fixed += q;
                e.gross += q.abs();
            }
        }
        e
    };
    let mut imbalance: f64 = 0.0;
    let mut emit = |time: f64, t: &[f64], energy: Energy, events: &mut dyn Events| {
        imbalance = imbalance.max(energy.imbalance().abs());
        let frame = Frame {
            time,
            temperature: t.to_vec(),
            flux: cell_flux(model, &grid, &mesh, &cell_of, t),
            probes: probes(model, &grid, &mids, &node_of, t)?,
            energy,
        };
        events.frame(&frame)
    };
    let mut kt = vec![0.0; n];
    let method;
    match model.step {
        Step::Steady => {
            let a = k.submatrix(&free);
            let solver = Solver::new(a, level, model.method, false)?;
            method = solver.method();
            events.progress(&format!("{} unknowns, {} solver", free.len(), name(method)));
            let rhs = reduced_rhs(&k, &load, &temperature, &free, None);
            let mut x = vec![0.0; free.len()];
            let stats = solver.solve(&rhs, &mut x, &|| events.cancelled())?;
            events.progress(&match stats.iterations {
                0 => format!("residual {:.1e}", stats.residual),
                i => format!("PCG {i} iterations, residual {:.1e}", stats.residual),
            });
            for (k, &i) in free.iter().enumerate() {
                temperature[i] = x[k];
            }
            k.mul(&temperature, &mut kt);
            let energy = energy_of(&temperature, &temperature, None, &kt);
            emit(0.0, &temperature, energy, events)?;
        }
        Step::Transient {
            end,
            dt,
            output_every,
            ..
        } => {
            let steps = (end / dt).round() as usize;
            let mut a_full = k.clone();
            for i in 0..n {
                let d = a_full.find(i, i).expect("a diagonal");
                a_full.val[d] += capacity[i] / dt;
            }
            let solver = Solver::new(a_full.submatrix(&free), level, model.method, true)?;
            method = solver.method();
            events.progress(&format!(
                "{} unknowns, {} solver, {steps} time steps",
                free.len(),
                name(method)
            ));
            // The first output has no rates: the heat flows start at time 0.
            emit(0.0, &temperature, Energy::default(), events)?;
            let mut x: Vec<f64> = free.iter().map(|&i| temperature[i]).collect();
            for s in 1..=steps {
                if events.cancelled() {
                    return Err("cancelled".into());
                }
                let previous = temperature.clone();
                let rhs = reduced_rhs(&a_full, &load, &temperature, &free, Some((&capacity, dt)));
                solver.solve(&rhs, &mut x, &|| events.cancelled())?;
                for (k, &i) in free.iter().enumerate() {
                    temperature[i] = x[k];
                }
                k.mul(&temperature, &mut kt);
                let energy = energy_of(&temperature, &previous, Some(dt), &kt);
                if s % output_every.max(1) == 0 || s == steps {
                    let time = s as f64 * dt;
                    events.progress(&format!("time {time:.6e} step {s} of {steps}"));
                    emit(time, &temperature, energy, events)?;
                }
            }
        }
    }
    if imbalance > IMBALANCE {
        warnings.push(format!(
            "the energy balance is off by {:.2e} of the heat flow: the linear solve is not exact",
            imbalance
        ));
    }
    Ok(Summary {
        cells: mesh.cells.len(),
        points: n,
        unknowns: free.len(),
        method,
        imbalance,
        warnings,
    })
}

fn name(method: Method) -> &'static str {
    match method {
        Method::Direct => "direct (sparse Cholesky)",
        Method::Multigrid => "iterative (PCG with multigrid)",
    }
}

/// The right side of the free unknowns: load - A T on the fixed points, with the capacity term
/// of a transient step.
fn reduced_rhs(
    a: &Csr,
    load: &[f64],
    t: &[f64],
    free: &[usize],
    transient: Option<(&[f64], f64)>,
) -> Vec<f64> {
    free.iter()
        .map(|&i| {
            let mut r = load[i];
            if let Some((c, dt)) = transient {
                r += c[i] / dt * t[i];
            }
            for e in a.ptr[i]..a.ptr[i + 1] {
                let j = a.idx[e] as usize;
                // Only the fixed points: their row of the free unknowns is not solved.
                if free.binary_search(&j).is_err() {
                    r -= a.val[e] * t[j];
                }
            }
            r
        })
        .collect()
}

/// The 8 x 8 conductivity matrix of a box cell of side lengths `s`, row-major, in the order of a
/// VTK hexahedron.
fn conduction(s: [f64; 3], k: [f64; 3]) -> [f64; 64] {
    const BITS: [[usize; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 1, 1],
    ];
    let stiff = |h: f64, a: usize, b: usize| if a == b { 1.0 / h } else { -1.0 / h };
    let mass = |h: f64, a: usize, b: usize| if a == b { h / 3.0 } else { h / 6.0 };
    let mut ke = [0.0; 64];
    for (p, bp) in BITS.iter().enumerate() {
        for (q, bq) in BITS.iter().enumerate() {
            let mut v = 0.0;
            for d in 0..3 {
                let mut term = k[d];
                for e in 0..3 {
                    term *= if e == d {
                        stiff(s[e], bp[e], bq[e])
                    } else {
                        mass(s[e], bp[e], bq[e])
                    };
                }
                v += term;
            }
            ke[p * 8 + q] = v;
        }
    }
    ke
}

/// The 4 x 4 convection matrix h * mass of a face of side lengths a and b, in the order of
/// `Grid::face_points`: (0, 0), (1, 0), (1, 1), (0, 1).
fn face_mass(a: f64, b: f64, h: f64) -> [f64; 16] {
    const BITS: [[usize; 2]; 4] = [[0, 0], [1, 0], [1, 1], [0, 1]];
    let mass = |l: f64, p: usize, q: usize| if p == q { l / 3.0 } else { l / 6.0 };
    let mut m = [0.0; 16];
    for (p, bp) in BITS.iter().enumerate() {
        for (q, bq) in BITS.iter().enumerate() {
            m[p * 4 + q] = h * mass(a, bp[0], bq[0]) * mass(b, bp[1], bq[1]);
        }
    }
    m
}

/// The heat flux -k grad T at the centre of each cell.
fn cell_flux(
    model: &Model,
    grid: &Grid,
    mesh: &Mesh,
    cell_of: &[usize],
    t: &[f64],
) -> Vec<[f64; 3]> {
    cell_of
        .iter()
        .zip(&mesh.cells)
        .map(|(&cell, nodes)| {
            let m = &model.materials[model.parts[grid.part[cell] as usize].material];
            let s = grid.size(cell);
            let v = nodes.map(|i| t[i as usize]);
            // The mean of the differences across each axis.
            let gx = (v[1] - v[0] + v[2] - v[3] + v[5] - v[4] + v[6] - v[7]) / (4.0 * s[0]);
            let gy = (v[3] - v[0] + v[2] - v[1] + v[7] - v[4] + v[6] - v[5]) / (4.0 * s[1]);
            let gz = (v[4] - v[0] + v[5] - v[1] + v[6] - v[2] + v[7] - v[3]) / (4.0 * s[2]);
            let k = m.conductivity;
            [-k[0] * gx, -k[1] * gy, -k[2] * gz]
        })
        .collect()
}

/// The temperature at each probe, by the trilinear interpolation of its cell. A probe in the
/// half that symmetry removed reads its mirror point.
fn probes(
    model: &Model,
    grid: &Grid,
    mids: &[Option<f64>; 3],
    node_of: &dyn Fn([usize; 3]) -> usize,
    t: &[f64],
) -> Result<Vec<f64>, String> {
    model
        .probes
        .iter()
        .map(|p| {
            let mut x = p.point;
            for d in 0..3 {
                if let Some(m) = mids[d] {
                    if x[d] < m {
                        x[d] = 2.0 * m - x[d];
                    }
                }
            }
            let (cell, l) = grid.locate(x).ok_or_else(|| {
                format!(
                    "probe {} at ({}, {}, {}) is in no part",
                    p.name, p.point[0], p.point[1], p.point[2]
                )
            })?;
            let corners = grid.corners(cell);
            let mut v = 0.0;
            for (c, corner) in corners.iter().enumerate() {
                let [bx, by, bz] = [
                    [0, 1, 1, 0, 0, 1, 1, 0][c],
                    [0, 0, 1, 1, 0, 0, 1, 1][c],
                    [0, 0, 0, 0, 1, 1, 1, 1][c],
                ];
                let w = |b: usize, f: f64| if b == 1 { f } else { 1.0 - f };
                v += w(bx, l[0]) * w(by, l[1]) * w(bz, l[2]) * t[node_of(*corner)];
            }
            Ok(v)
        })
        .collect()
}

/// The checks of the model that the grid does not need.
fn check(model: &Model) -> Result<(), String> {
    for m in &model.materials {
        if m.conductivity.iter().any(|k| !(k.is_finite() && *k > 0.0)) {
            return Err(format!(
                "material {}: the conductivity must be above 0",
                m.name
            ));
        }
        if !(m.capacity.is_finite() && m.capacity >= 0.0) {
            return Err(format!(
                "material {}: the density and the specific heat must be 0 or more",
                m.name
            ));
        }
    }
    for f in &model.faces {
        if let Some((lo, hi)) = f.region {
            if (0..3).any(|d| !(lo[d] <= hi[d])) {
                return Err(format!("face set {}: the box must have lo <= hi", f.name));
            }
        }
    }
    for load in &model.loads {
        let finite = match load {
            Load::Temperature { value, .. }
            | Load::Flux { value, .. }
            | Load::Source { value, .. } => value.is_finite(),
            Load::Convection { film, sink, .. } => {
                if !(*film > 0.0) {
                    return Err("a convection needs a film coefficient above 0".into());
                }
                film.is_finite() && sink.is_finite()
            }
        };
        if !finite {
            return Err("each load needs finite numbers".into());
        }
    }
    if let Step::Transient {
        end,
        dt,
        output_every,
        initial,
    } = model.step
    {
        if !(dt > 0.0 && end > 0.0 && dt.is_finite() && end.is_finite() && initial.is_finite()) {
            return Err(
                "a transient step needs end and dt above 0 and a finite initial temperature".into(),
            );
        }
        let steps = (end / dt).round();
        if steps < 1.0 || (steps * dt - end).abs() > 1e-9 * end {
            return Err(format!(
                "the end time {end} must be a whole number of time steps dt = {dt}"
            ));
        }
        if output_every == 0 {
            return Err("output_every must be 1 or more".into());
        }
        for p in &model.parts {
            let m = &model.materials[p.material];
            if !(m.capacity > 0.0) {
                return Err(format!(
                    "part {}: a transient step needs the density and the specific heat of \
                     material {}",
                    p.name, m.name
                ));
            }
        }
    }
    Ok(())
}

/// The faces of each face set.
type FaceSets = Vec<Vec<Face>>;

/// The grid, the faces of each face set and the middle plane of each axis of symmetry. With
/// symmetry, the grid is the cut grid, and the checks of the mirror are on the full grid.
fn build_grid(model: &Model) -> Result<(Grid, FaceSets, [Option<f64>; 3]), String> {
    let mut mids = [None; 3];
    for d in 0..3 {
        if model.symmetry[d] {
            let lo = model
                .parts
                .iter()
                .map(|p| p.lo[d])
                .fold(f64::INFINITY, f64::min);
            let hi = model
                .parts
                .iter()
                .map(|p| p.hi[d])
                .fold(f64::NEG_INFINITY, f64::max);
            mids[d] = Some(0.5 * (lo + hi));
        }
    }
    let extra: [Vec<f64>; 3] = std::array::from_fn(|d| mids[d].into_iter().collect());
    let grid = Grid::build(
        &model.parts,
        model.spacing,
        [&extra[0], &extra[1], &extra[2]],
    )?;
    let all = grid.faces();
    let faces: Vec<Vec<Face>> = model
        .faces
        .iter()
        .map(|set| select(&grid, &all, set))
        .collect();
    for (set, list) in model.faces.iter().zip(&faces) {
        let used = model.loads.iter().any(|l| match l {
            Load::Temperature { faces, .. }
            | Load::Flux { faces, .. }
            | Load::Convection { faces, .. } => model.faces[*faces].name == set.name,
            Load::Source { .. } => false,
        });
        if used && list.is_empty() {
            return Err(format!(
                "face set {} selects no face of the model: a face of the model has no part beyond it",
                set.name
            ));
        }
    }
    let mut grid = grid;
    let mut faces = faces;
    for d in 0..3 {
        let Some(mid) = mids[d] else { continue };
        let axis = ["x", "y", "z"][d];
        let refuse = |what: String| {
            Err(format!(
                "symmetry at {axis} = {mid}: {what} do not mirror at this plane. Solve the full model"
            ))
        };
        if !grid.planes_mirror(d, mid) {
            return refuse("the grid planes".into());
        }
        // A cell and its mirror have the same material and the same heat source.
        let mut source = vec![0.0; model.parts.len()];
        for l in &model.loads {
            if let Load::Source { part, value } = l {
                source[*part] += value;
            }
        }
        let sign = |c: usize| -> Option<(usize, u64)> {
            let p = grid.part[c] as usize;
            (p != VOID as usize).then(|| (model.parts[p].material, source[p].to_bits()))
        };
        if (0..grid.part.len()).any(|c| sign(c) != sign(grid.mirror_cell(c, d))) {
            return refuse("the materials or the heat sources of the parts".into());
        }
        for (set, list) in model.faces.iter().zip(&faces) {
            let mut sorted = list.clone();
            sorted.sort_unstable();
            let mirrored = list.iter().all(|f| {
                let side = Side::ALL[f.side as usize];
                let side = if side.axis == d {
                    Side {
                        positive: !side.positive,
                        ..side
                    }
                } else {
                    side
                };
                let m = Face {
                    cell: grid.mirror_cell(f.cell, d),
                    side: Side::ALL.iter().position(|s| *s == side).expect("a side") as u8,
                };
                sorted.binary_search(&m).is_ok()
            });
            if !mirrored {
                return refuse(format!("the faces of face set {}", set.name));
            }
        }
        let (cut, old) = grid.crop(d, mid);
        let mut new = vec![usize::MAX; grid.part.len()];
        for (k, &o) in old.iter().enumerate() {
            new[o] = k;
        }
        for list in &mut faces {
            *list = list
                .iter()
                .filter(|f| new[f.cell] != usize::MAX)
                .map(|f| Face {
                    cell: new[f.cell],
                    side: f.side,
                })
                .collect();
        }
        grid = cut;
    }
    Ok((grid, faces, mids))
}

/// The faces of a face set.
fn select(grid: &Grid, all: &[Face], set: &FaceSet) -> Vec<Face> {
    all.iter()
        .copied()
        .filter(|&f| {
            let side = Side::ALL[f.side as usize];
            if !set.sides.is_empty() && !set.sides.contains(&side) {
                return false;
            }
            if !set.parts.is_empty() && !set.parts.contains(&(grid.part[f.cell] as usize)) {
                return false;
            }
            if let Some((lo, hi)) = set.region {
                let c = grid.face_centre(f);
                for d in 0..3 {
                    let t = tol(&grid.planes[d]);
                    if c[d] < lo[d] - t || c[d] > hi[d] + t {
                        return false;
                    }
                }
            }
            true
        })
        .collect()
}

/// A steady model needs a fixed temperature or a convection on each connected piece, else its
/// temperature has no unique value.
/// `held` marks each point with a fixed temperature or a convection.
fn check_supports(model: &Model, mesh: &Mesh, held: &[bool]) -> Result<(), String> {
    let n = mesh.points.len();
    let mut root: Vec<usize> = (0..n).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for c in &mesh.cells {
        let a = find(&mut root, c[0] as usize);
        for &v in &c[1..] {
            let b = find(&mut root, v as usize);
            root[b] = a;
        }
    }
    let mut piece_held = vec![false; n];
    for i in (0..n).filter(|&i| held[i]) {
        let r = find(&mut root, i);
        piece_held[r] = true;
    }
    for (e, c) in mesh.cells.iter().enumerate() {
        let r = find(&mut root, c[0] as usize);
        if !piece_held[r] {
            let part = &model.parts[mesh.part[e] as usize].name;
            return Err(format!(
                "part {part} has no fixed temperature and no convection on it or on a part that \
                 it touches, so a steady temperature has no unique value: add a fixed temperature \
                 or a convection, or use a transient step"
            ));
        }
    }
    Ok(())
}
