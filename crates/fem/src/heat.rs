//! Steady and transient heat conduction on the box grid with 8-node hex elements, or on the
//! 2.5D tetrahedral mesh (`prism`) with 4-node tetrahedra when a part has a footprint.
//!
//! The model is linear: each material has a constant conductivity (1 value, or 1 value for each
//! axis), density and specific heat. The loads are a fixed temperature, a heat flux into the
//! body and a convection on named faces, and a heat source in the volume of a part. Parts that
//! touch share their nodes, so the heat crosses with no contact resistance. A transient step is
//! backward Euler with a lumped capacity, so a large step gives no oscillation.
//!
//! The element matrices of a box cell are exact tensor products of the 1D matrices of the
//! linear element: the conductivity k/h [1 -1; -1 1] and the mass h/6 [2 1; 1 2]. A linear
//! tetrahedron has a constant gradient, so its conductivity matrix is V G^T K G, with the
//! gradients G of its 4 shape functions, and a triangle face has the mass A/12 [2 1 1; 1 2 1;
//! 1 1 2]. Each discretization gives the same set of matrices and face lists to 1 solve loop.
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
//! conditions); O. C. Zienkiewicz, R. L. Taylor and J. Z. Zhu, The Finite Element Method: Its
//! Basis and Fundamentals, 7th ed., Elsevier 2013, chapter 6 (the linear tetrahedron).

use crate::grid::{tol, Face, Grid, Part, Side, Spacing, VOID};
use crate::multigrid::Level;
use crate::prism::{self, BoundaryFace, TetMesh};
use crate::solve::{
    Crossover, Method, Solver, Unknowns, STEADY_DIRECT_LIMIT, TRANSIENT_DIRECT_LIMIT,
};
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
    /// The faces on a circle outline of a footprint: its centre and radius, across the
    /// extrusion axis (`None`: all).
    pub circle: Option<([f64; 2], f64)>,
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
    /// The extrusion axis of the parts with a footprint (0 x, 1 y, 2 z).
    pub axis: usize,
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

/// The kind of the cells of a mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    /// An 8-node box, in the order of a VTK hexahedron.
    Hex8,
    /// A 4-node tetrahedron with a positive volume, in the order of a VTK tetra.
    Tet4,
}

impl Cell {
    pub fn nodes(self) -> usize {
        match self {
            Cell::Hex8 => 8,
            Cell::Tet4 => 4,
        }
    }
}

/// The mesh of the result.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub points: Vec<[f64; 3]>,
    pub kind: Cell,
    /// The points of the cells, `kind.nodes()` for each cell.
    pub cells: Vec<u32>,
    /// The part of each cell.
    pub part: Vec<u32>,
}

impl Mesh {
    /// The number of cells.
    pub fn len(&self) -> usize {
        self.part.len()
    }

    pub fn is_empty(&self) -> bool {
        self.part.is_empty()
    }

    /// The points of cell `e`.
    pub fn cell(&self, e: usize) -> &[u32] {
        let n = self.kind.nodes();
        &self.cells[e * n..(e + 1) * n]
    }
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

/// The most points of a grid and of a mesh of tetrahedra, and the memory that they need, for the
/// error of a larger model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit {
    pub points: usize,
    pub tet_points: usize,
    pub memory: &'static str,
}

/// A grid with more points than this needs more memory than a laptop has. Reason: a steady
/// multigrid solve of 1,009,057 points used 1.9 GB (the peak resident size), so 4,000,000 points
/// need about 7.5 GB, half of the RAM of a 16 GB laptop.
pub const LAPTOP: Limit = Limit {
    points: 4_000_000,
    tet_points: MAX_TET_POINTS,
    memory: "the memory of a laptop",
};

/// A tetrahedral mesh with more points than this takes minutes, because its solver is the
/// direct solver. Reason: a steady solve of a solid cylinder (Apple M-series, 1 thread) took
/// 9.9 s and 1.7 GB at 154,440 points, and 97 s and 2.7 GB at 336,141 points. The time grows as
/// about n^2.2, so 400,000 points take about 2.5 minutes.
const MAX_TET_POINTS: usize = 400_000;

/// An energy imbalance above this fraction is a warning.
const IMBALANCE: f64 = 1e-6;

/// A face on the outside of the model, with a load on it.
struct Facet {
    /// The points: 4 of a box face, or 3 of a triangle (the 4th is unused).
    nodes: [u32; 4],
    count: usize,
    area: f64,
    /// The mass matrix of the face (the integral of N_i N_j), `count` x `count`, row-major.
    mass: Vec<f64>,
}

impl Facet {
    fn nodes(&self) -> &[u32] {
        &self.nodes[..self.count]
    }
}

/// The heat flux at the centre of each cell, from the point temperatures.
type FluxOf = Box<dyn Fn(&[f64]) -> Vec<[f64; 3]>>;

/// The unknowns of the linear solver from the free points.
type UnknownsOf = Box<dyn Fn(&[usize]) -> Unknowns>;

/// A model on its mesh: what the solve loop needs from the box grid or the tetrahedral mesh.
struct Discrete {
    mesh: Mesh,
    /// The conductivity matrix.
    k: Csr,
    /// The volume of each cell.
    volume: Vec<f64>,
    /// The faces of each face set of the model.
    faces: Vec<Vec<Facet>>,
    /// The points and the weights of the interpolation of each probe.
    probes: Vec<Vec<(u32, f64)>>,
    flux: FluxOf,
    unknowns: UnknownsOf,
    /// The line that tells the mesh.
    line: String,
    warnings: Vec<String>,
}

/// Solve the model on a laptop.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Summary, String> {
    solve_within(model, LAPTOP, events)
}

/// Solve the model, and refuse a grid with more points than `limit`.
pub fn solve_within(
    model: &Model,
    limit: Limit,
    events: &mut dyn Events,
) -> Result<Summary, String> {
    check(model)?;
    let d = if model.parts.iter().any(|p| p.footprint.is_some()) {
        tet_model(model, limit)?
    } else {
        grid_model(model, limit)?
    };
    run(model, d, events)
}

/// The steady solve or the time steps of the model `model` on its mesh `d`.
fn run(model: &Model, d: Discrete, events: &mut dyn Events) -> Result<Summary, String> {
    let Discrete {
        mesh,
        mut k,
        volume,
        faces,
        probes,
        flux: cell_flux,
        unknowns,
        line,
        mut warnings,
    } = d;
    events.mesh(&mesh)?;
    events.progress(&line);
    let n = mesh.points.len();
    let per = mesh.kind.nodes() as f64;
    // The lumped capacity and the volume sources.
    let mut capacity = vec![0.0; n];
    let mut source = vec![0.0; n];
    let mut part_source = vec![0.0; model.parts.len()];
    for load in &model.loads {
        if let Load::Source { part, value } = load {
            part_source[*part] += value;
        }
    }
    for e in 0..mesh.len() {
        let p = mesh.part[e] as usize;
        let m = &model.materials[model.parts[p].material];
        for &node in mesh.cell(e) {
            capacity[node as usize] += m.capacity * volume[e] / per;
            source[node as usize] += part_source[p] * volume[e] / per;
        }
    }
    let mut flux = vec![0.0; n];
    let mut film = vec![0.0; n];
    let mut fixed: Vec<Option<(f64, usize)>> = vec![None; n];
    // The convection faces: the nodes, the area, the film coefficient and the sink.
    let mut convection: Vec<(&[u32], f64, f64, f64)> = Vec::new();
    for load in &model.loads {
        let set = match load {
            Load::Source { .. } => continue,
            Load::Temperature { faces, .. }
            | Load::Flux { faces, .. }
            | Load::Convection { faces, .. } => *faces,
        };
        for face in &faces[set] {
            let nodes = face.nodes();
            let share = face.area / face.count as f64;
            match load {
                Load::Temperature { value, faces: f } => {
                    for &node in nodes {
                        match fixed[node as usize] {
                            Some((v, other)) if v != *value => {
                                return Err(format!(
                                    "the face sets {} and {} share a point with 2 fixed \
                                     temperatures, {v} and {value}",
                                    model.faces[other].name, model.faces[*f].name
                                ))
                            }
                            _ => fixed[node as usize] = Some((*value, *f)),
                        }
                    }
                }
                Load::Flux { value, .. } => {
                    for &node in nodes {
                        flux[node as usize] += value * share;
                    }
                }
                Load::Convection {
                    film: h, sink: t, ..
                } => {
                    let hm: Vec<f64> = face.mass.iter().map(|m| h * m).collect();
                    k.add_element(nodes, &hm);
                    for &node in nodes {
                        film[node as usize] += h * t * share;
                    }
                    convection.push((nodes, face.area, *h, *t));
                }
                Load::Source { .. } => unreachable!("skipped above"),
            }
        }
    }
    let transient = matches!(model.step, Step::Transient { .. });
    if !transient {
        let mut held: Vec<bool> = fixed.iter().map(Option::is_some).collect();
        for (nodes, ..) in &convection {
            for &v in *nodes {
                held[v as usize] = true;
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
    let energy_of = |t: &[f64], previous: &[f64], dt: Option<f64>, kt: &[f64]| -> Energy {
        let mut e = Energy {
            source: source.iter().sum(),
            flux: flux.iter().sum(),
            gross: source.iter().chain(&flux).map(|v| v.abs()).sum(),
            ..Energy::default()
        };
        for (nodes, area, h, sink) in &convection {
            let mean = nodes.iter().map(|&v| t[v as usize]).sum::<f64>() / nodes.len() as f64;
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
            flux: cell_flux(t),
            probes: probes
                .iter()
                .map(|w| w.iter().map(|&(v, f)| f * t[v as usize]).sum())
                .collect(),
            energy,
        };
        events.frame(&frame)
    };
    let mut kt = vec![0.0; n];
    let method;
    match model.step {
        Step::Steady => {
            let a = k.submatrix(&free);
            let solver = Solver::new(
                a,
                unknowns(&free),
                model.method,
                Crossover::Unknowns(STEADY_DIRECT_LIMIT),
                None,
            )?;
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
            let solver = Solver::new(
                a_full.submatrix(&free),
                unknowns(&free),
                model.method,
                Crossover::Unknowns(TRANSIENT_DIRECT_LIMIT),
                None,
            )?;
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
        cells: mesh.len(),
        points: n,
        unknowns: free.len(),
        method,
        imbalance,
        warnings,
    })
}

/// The model on the box grid, with hex cells.
fn grid_model(model: &Model, limit: Limit) -> Result<Discrete, String> {
    if let Some(f) = model.faces.iter().find(|f| f.circle.is_some()) {
        return Err(format!(
            "face set {}: `circle` selects the faces on a circle outline, and no part has one",
            f.name
        ));
    }
    let mut warnings = Vec::new();
    let used: Vec<bool> = model
        .faces
        .iter()
        .enumerate()
        .map(|(f, _)| {
            model.loads.iter().any(|l| match l {
                Load::Temperature { faces, .. }
                | Load::Flux { faces, .. }
                | Load::Convection { faces, .. } => *faces == f,
                Load::Source { .. } => false,
            })
        })
        .collect();
    let mut source = vec![0.0f64; model.parts.len()];
    for l in &model.loads {
        if let Load::Source { part, value } = l {
            source[*part] += value;
        }
    }
    let keys: Vec<Vec<u64>> = source.iter().map(|q| vec![q.to_bits()]).collect();
    let (grid, faces, mids) = build_grid(
        &model.parts,
        model.spacing,
        model.symmetry,
        &model.faces,
        &used,
        &keys,
        "the materials or the heat sources of the parts",
    )?;
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
    let Numbering {
        mesh,
        ijk,
        cell_of,
        number,
    } = number(&grid, limit)?;
    let n = ijk.len();
    let dims = grid.dims();
    let line = format!(
        "grid {} x {} x {} planes, {} cells, {} points",
        dims[0] + 1,
        dims[1] + 1,
        dims[2] + 1,
        mesh.len(),
        n
    );
    let node_of = |p: [usize; 3]| number[grid.point_index(p)];
    let mut k = Csr::pattern(n, (0..mesh.len()).map(|e| mesh.cell(e)));
    let mut volume = Vec::with_capacity(mesh.len());
    for (e, &cell) in cell_of.iter().enumerate() {
        let m = &model.materials[model.parts[grid.part[cell] as usize].material];
        let size = grid.size(cell);
        k.add_element(mesh.cell(e), &conduction(size, m.conductivity));
        volume.push(size[0] * size[1] * size[2]);
    }
    let facets: Vec<Vec<Facet>> = faces
        .iter()
        .map(|list| {
            list.iter()
                .map(|&face| {
                    let s = grid.size(face.cell);
                    let side = Side::ALL[face.side as usize];
                    let (a, b) = match side.axis {
                        0 => (s[1], s[2]),
                        1 => (s[0], s[2]),
                        _ => (s[0], s[1]),
                    };
                    Facet {
                        nodes: grid.face_points(face).map(node_of),
                        count: 4,
                        area: grid.face_area(face),
                        mass: face_mass(a, b, 1.0).to_vec(),
                    }
                })
                .collect()
        })
        .collect();
    let probes = probes(model, &grid, &mids, &node_of)?;
    let level = Level {
        planes: grid.planes.clone(),
        points: Vec::new(),
        comp: Vec::new(),
    };
    let conductivity: Vec<[f64; 3]> = cell_of
        .iter()
        .map(|&c| model.materials[model.parts[grid.part[c] as usize].material].conductivity)
        .collect();
    let sizes: Vec<[f64; 3]> = cell_of.iter().map(|&c| grid.size(c)).collect();
    let cells_flat = mesh.cells.clone();
    Ok(Discrete {
        mesh,
        k,
        volume,
        faces: facets,
        probes,
        flux: Box::new(move |t| cell_flux(&cells_flat, &sizes, &conductivity, t)),
        unknowns: Box::new(move |free| {
            Unknowns::Grid(Level {
                planes: level.planes.clone(),
                points: free.iter().map(|&i| ijk[i]).collect(),
                comp: vec![0; free.len()],
            })
        }),
        line,
        warnings,
    })
}

/// The model on the 2.5D tetrahedral mesh.
fn tet_model(model: &Model, limit: Limit) -> Result<Discrete, String> {
    if model.symmetry.iter().any(|&s| s) {
        return Err(
            "symmetry needs a model of boxes: a model with a polygon or a circle part is solved \
             in full"
                .into(),
        );
    }
    let why = if limit.tet_points < MAX_TET_POINTS {
        format!("{} solves", limit.memory)
    } else {
        "the direct solver solves in a few minutes".to_string()
    };
    let refusal = format!(
        "the mesh has more than {} points, the most that {why}: use a larger max_cell or a larger \
         max_growth",
        limit.tet_points
    );
    let tm = prism::mesh(
        &model.parts,
        model.axis,
        model.spacing,
        limit.tet_points,
        &refusal,
        false,
    )?;
    let n = tm.points.len();
    let mesh = Mesh {
        points: tm.points.clone(),
        kind: Cell::Tet4,
        cells: tm.tets.iter().flatten().copied().collect(),
        part: tm.part.clone(),
    };
    let line = format!(
        "mesh {} triangles x {} planes, {} tetrahedra, {} points",
        tm.triangles,
        tm.planes,
        mesh.len(),
        n
    );
    let mut k = Csr::pattern(n, (0..mesh.len()).map(|e| mesh.cell(e)));
    let mut volume = Vec::with_capacity(mesh.len());
    // The gradients of the 4 shape functions of each tetrahedron, times its conductivity.
    let mut grads: Vec<[[f64; 3]; 4]> = Vec::with_capacity(mesh.len());
    for (e, tet) in tm.tets.iter().enumerate() {
        let c = model.materials[model.parts[tm.part[e] as usize].material].conductivity;
        let (g, v) = tet_gradients(tet.map(|i| tm.points[i as usize]));
        let mut ke = [0.0; 16];
        for i in 0..4 {
            for j in 0..4 {
                ke[i * 4 + j] = v * (0..3).map(|d| c[d] * g[i][d] * g[j][d]).sum::<f64>();
            }
        }
        k.add_element(tet, &ke);
        volume.push(v);
        grads.push(g.map(|gi| std::array::from_fn(|d| c[d] * gi[d])));
    }
    let faces = tet_faces(model, &tm)?;
    let probes = model
        .probes
        .iter()
        .map(|p| {
            locate_tet(&tm, p.point).ok_or_else(|| {
                format!(
                    "probe {} at ({}, {}, {}) is in no part",
                    p.name, p.point[0], p.point[1], p.point[2]
                )
            })
        })
        .collect::<Result<_, _>>()?;
    let tets = tm.tets.clone();
    let points = tm.points;
    Ok(Discrete {
        mesh,
        k,
        volume,
        faces,
        probes,
        flux: Box::new(move |t| {
            tets.iter()
                .zip(&grads)
                .map(|(tet, g)| {
                    std::array::from_fn(|d| {
                        -(0..4).map(|i| g[i][d] * t[tet[i] as usize]).sum::<f64>()
                    })
                })
                .collect()
        }),
        unknowns: Box::new(move |free| Unknowns::Points(free.iter().map(|&i| points[i]).collect())),
        line,
        warnings: Vec::new(),
    })
}

/// The gradients of the 4 shape functions of a tetrahedron and its volume.
pub(crate) fn tet_gradients(p: [[f64; 3]; 4]) -> ([[f64; 3]; 4], f64) {
    let e: [[f64; 3]; 3] = std::array::from_fn(|k| std::array::from_fn(|d| p[k + 1][d] - p[0][d]));
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let det = e[0][0] * (e[1][1] * e[2][2] - e[1][2] * e[2][1])
        - e[0][1] * (e[1][0] * e[2][2] - e[1][2] * e[2][0])
        + e[0][2] * (e[1][0] * e[2][1] - e[1][1] * e[2][0]);
    // The rows of the inverse of the matrix with the columns e are the gradients of N1..N3.
    let rows = [cross(e[1], e[2]), cross(e[2], e[0]), cross(e[0], e[1])];
    let g: [[f64; 3]; 3] = rows.map(|r| r.map(|v| v / det));
    let g0 = std::array::from_fn(|d| -(g[0][d] + g[1][d] + g[2][d]));
    ([g0, g[0], g[1], g[2]], det / 6.0)
}

/// The points of the tetrahedron that holds `x`, with the barycentric weight of each. On a
/// face between 2 tetrahedra, either one.
fn locate_tet(tm: &TetMesh, x: [f64; 3]) -> Option<Vec<(u32, f64)>> {
    // The tetrahedron where the smallest weight is largest: inside, all weights are 0 or more.
    let mut best: Option<(f64, [f64; 4], usize)> = None;
    for (e, tet) in tm.tets.iter().enumerate() {
        let p = tet.map(|i| tm.points[i as usize]);
        let (g, _) = tet_gradients(p);
        let mut w = [0.0; 4];
        for i in 1..4 {
            w[i] = (0..3).map(|d| g[i][d] * (x[d] - p[0][d])).sum();
        }
        w[0] = 1.0 - w[1] - w[2] - w[3];
        let worst = w.iter().copied().fold(f64::INFINITY, f64::min);
        if best.is_none_or(|(b, ..)| worst > b) {
            best = Some((worst, w, e));
        }
    }
    let (worst, w, e) = best?;
    (worst >= -1e-9).then(|| tm.tets[e].iter().copied().zip(w).collect())
}

/// The size of the bounding box of the points times 1e-9: the tolerance of a face selection.
pub(crate) fn face_tol(points: &[[f64; 3]]) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for d in 0..3 {
            lo[d] = lo[d].min(p[d]);
            hi[d] = hi[d].max(p[d]);
        }
    }
    1e-9 * (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max)
}

/// The area of the boundary face `f` when the face set `set` holds it, else `None`. The part of
/// a face is the part of its tetrahedron, or the hollow part beyond it. `t` is the tolerance of
/// `face_tol`.
pub(crate) fn selects(set: &FaceSet, tm: &TetMesh, f: &BoundaryFace, t: f64) -> Option<f64> {
    let p = f.nodes.map(|i| tm.points[i as usize]);
    let u: [f64; 3] = std::array::from_fn(|d| p[1][d] - p[0][d]);
    let w: [f64; 3] = std::array::from_fn(|d| p[2][d] - p[0][d]);
    let n = [
        u[1] * w[2] - u[2] * w[1],
        u[2] * w[0] - u[0] * w[2],
        u[0] * w[1] - u[1] * w[0],
    ];
    let twice = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let unit = n.map(|v| v / twice);
    if !set.sides.is_empty()
        && !set.sides.iter().any(|s| {
            let sign = if s.positive { 1.0 } else { -1.0 };
            unit[s.axis] * sign > 1.0 - 1e-9
        })
    {
        return None;
    }
    if !set.parts.is_empty()
        && !set.parts.contains(&(tm.part[f.tet as usize] as usize))
        && !set.parts.contains(&(f.beyond as usize))
    {
        return None;
    }
    if let Some((blo, bhi)) = set.region {
        let c: [f64; 3] = std::array::from_fn(|d| (p[0][d] + p[1][d] + p[2][d]) / 3.0);
        if !(0..3).all(|d| c[d] >= blo[d] - t && c[d] <= bhi[d] + t) {
            return None;
        }
    }
    if let Some((c, r)) = set.circle {
        let on = f.circles.iter().any(|&k| {
            let (cc, rr) = tm.circles[k as usize];
            (cc[0] - c[0]).abs() <= t && (cc[1] - c[1]).abs() <= t && (rr - r).abs() <= t
        });
        if !on {
            return None;
        }
    }
    Some(0.5 * twice)
}

/// The faces of each face set of the model on the tetrahedral mesh.
fn tet_faces(model: &Model, tm: &TetMesh) -> Result<Vec<Vec<Facet>>, String> {
    let t = face_tol(&tm.points);
    let mut out = Vec::new();
    for set in &model.faces {
        let mut list = Vec::new();
        for f in &tm.faces {
            let Some(area) = selects(set, tm, f, t) else {
                continue;
            };
            let m = area / 12.0;
            list.push(Facet {
                nodes: [f.nodes[0], f.nodes[1], f.nodes[2], 0],
                count: 3,
                area,
                mass: vec![2.0 * m, m, m, m, 2.0 * m, m, m, m, 2.0 * m],
            });
        }
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
        out.push(list);
    }
    Ok(out)
}

/// The mesh of the cells of the parts of a grid, and the maps between the grid and the mesh.
pub(crate) struct Numbering {
    pub mesh: Mesh,
    /// The grid point of each point of the mesh.
    pub ijk: Vec<[u32; 3]>,
    /// The grid cell of each cell of the mesh.
    pub cell_of: Vec<usize>,
    /// The mesh point of each grid point (`Grid::point_index`), or `u32::MAX`.
    pub number: Vec<u32>,
}

/// Number the points of the cells of the parts. An error tells that the grid has more than
/// `limit.points` points.
pub(crate) fn number(grid: &Grid, limit: Limit) -> Result<Numbering, String> {
    let dims = grid.dims();
    let mut number = vec![u32::MAX; (dims[0] + 1) * (dims[1] + 1) * (dims[2] + 1)];
    let mut ijk: Vec<[u32; 3]> = Vec::new();
    let mut cells: Vec<u32> = Vec::new();
    let mut cell_of: Vec<usize> = Vec::new();
    for cell in 0..grid.part.len() {
        if grid.part[cell] == VOID {
            continue;
        }
        for p in grid.corners(cell) {
            let slot = &mut number[grid.point_index(p)];
            if *slot == u32::MAX {
                *slot = ijk.len() as u32;
                ijk.push([p[0] as u32, p[1] as u32, p[2] as u32]);
                if ijk.len() > limit.points {
                    return Err(format!(
                        "the grid has more than {} points, the most that {} solves: use a \
                         larger max_cell or a larger max_growth",
                        limit.points, limit.memory
                    ));
                }
            }
            cells.push(*slot);
        }
        cell_of.push(cell);
    }
    let points: Vec<[f64; 3]> = ijk
        .iter()
        .map(|p| grid.point([p[0] as usize, p[1] as usize, p[2] as usize]))
        .collect();
    let mesh = Mesh {
        points,
        kind: Cell::Hex8,
        cells,
        part: cell_of.iter().map(|&c| grid.part[c]).collect(),
    };
    Ok(Numbering {
        mesh,
        ijk,
        cell_of,
        number,
    })
}

pub(crate) fn name(method: Method) -> &'static str {
    match method {
        Method::Direct => "direct (sparse Cholesky)",
        Method::Multigrid => "iterative (PCG with multigrid)",
    }
}

/// The right side of the free unknowns: load - A T on the fixed points, with the capacity term
/// of a transient step.
pub(crate) fn reduced_rhs(
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
pub(crate) fn conduction(s: [f64; 3], k: [f64; 3]) -> [f64; 64] {
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

/// The heat flux -k grad T at the centre of each box cell.
fn cell_flux(cells: &[u32], sizes: &[[f64; 3]], k: &[[f64; 3]], t: &[f64]) -> Vec<[f64; 3]> {
    cells
        .chunks_exact(8)
        .zip(sizes)
        .zip(k)
        .map(|((nodes, s), k)| {
            let v: Vec<f64> = nodes.iter().map(|&i| t[i as usize]).collect();
            // The mean of the differences across each axis.
            let gx = (v[1] - v[0] + v[2] - v[3] + v[5] - v[4] + v[6] - v[7]) / (4.0 * s[0]);
            let gy = (v[3] - v[0] + v[2] - v[1] + v[7] - v[4] + v[6] - v[5]) / (4.0 * s[1]);
            let gz = (v[4] - v[0] + v[5] - v[1] + v[6] - v[2] + v[7] - v[3]) / (4.0 * s[2]);
            [-k[0] * gx, -k[1] * gy, -k[2] * gz]
        })
        .collect()
}

/// The points and the trilinear weights of each probe in its cell. A probe in the half that
/// symmetry removed reads its mirror point.
fn probes(
    model: &Model,
    grid: &Grid,
    mids: &[Option<f64>; 3],
    node_of: &dyn Fn([usize; 3]) -> u32,
) -> Result<Vec<Vec<(u32, f64)>>, String> {
    model
        .probes
        .iter()
        .map(|p| {
            let (cell, l, _) = locate_probe(grid, mids, p)?;
            let w = crate::hex::shape(l.map(|f| 2.0 * f - 1.0));
            Ok(grid
                .corners(cell)
                .iter()
                .zip(w)
                .map(|(corner, w)| (node_of(*corner), w))
                .collect())
        })
        .collect()
}

/// The cell of a probe, the local coordinates (0 to 1) of the probe in it, and the axes on which
/// the probe is in the half that symmetry removed: it reads its mirror point.
pub(crate) fn locate_probe(
    grid: &Grid,
    mids: &[Option<f64>; 3],
    p: &Probe,
) -> Result<(usize, [f64; 3], [bool; 3]), String> {
    let mut x = p.point;
    let mut mirrored = [false; 3];
    for d in 0..3 {
        if let Some(m) = mids[d] {
            if x[d] < m {
                x[d] = 2.0 * m - x[d];
                mirrored[d] = true;
            }
        }
    }
    let (cell, l) = grid.locate(x).ok_or_else(|| {
        format!(
            "probe {} at ({}, {}, {}) is in no part",
            p.name, p.point[0], p.point[1], p.point[2]
        )
    })?;
    Ok((cell, l, mirrored))
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
        if let Some((c, r)) = f.circle {
            if !(c.iter().all(|v| v.is_finite()) && r.is_finite() && r > 0.0) {
                return Err(format!(
                    "face set {}: a circle needs a finite centre and a radius above 0",
                    f.name
                ));
            }
        }
    }
    if model.axis > 2 {
        return Err("the extrusion axis must be x, y or z".into());
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
/// symmetry, the grid is the cut grid, and the checks of the mirror are on the full grid: a cell
/// and its mirror have the same material and the same `keys` of their parts (for example the
/// heat source), and each face set mirrors. `used` marks the face sets that a load uses: each
/// must select a face. `what` names the keys in the error of a model that does not mirror.
pub(crate) fn build_grid(
    parts: &[Part],
    spacing: Spacing,
    symmetry: [bool; 3],
    face_sets: &[FaceSet],
    used: &[bool],
    keys: &[Vec<u64>],
    what: &str,
) -> Result<(Grid, FaceSets, [Option<f64>; 3]), String> {
    let mut mids = [None; 3];
    for d in 0..3 {
        if symmetry[d] {
            let lo = parts.iter().map(|p| p.lo[d]).fold(f64::INFINITY, f64::min);
            let hi = parts
                .iter()
                .map(|p| p.hi[d])
                .fold(f64::NEG_INFINITY, f64::max);
            mids[d] = Some(0.5 * (lo + hi));
        }
    }
    let extra: [Vec<f64>; 3] = std::array::from_fn(|d| mids[d].into_iter().collect());
    let grid = Grid::build(parts, spacing, [&extra[0], &extra[1], &extra[2]])?;
    let all = grid.faces();
    let faces: Vec<Vec<Face>> = face_sets
        .iter()
        .map(|set| select(&grid, &all, set))
        .collect();
    for ((set, list), used) in face_sets.iter().zip(&faces).zip(used) {
        if *used && list.is_empty() {
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
        let sign = |c: usize| -> Option<(usize, &[u64])> {
            let p = grid.part[c] as usize;
            (p != VOID as usize).then(|| (parts[p].material, &keys[p][..]))
        };
        if (0..grid.part.len()).any(|c| sign(c) != sign(grid.mirror_cell(c, d))) {
            return refuse(what.into());
        }
        for (set, list) in face_sets.iter().zip(&faces) {
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
    for e in 0..mesh.len() {
        let c = mesh.cell(e);
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
    for e in 0..mesh.len() {
        let r = find(&mut root, mesh.cell(e)[0] as usize);
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
