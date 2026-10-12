//! Build the linear system from a model, eliminate constraints and solve.
//!
//! Steps are linear. Each step solves once. Boundary conditions, concentrated
//! loads and pressure loads carry over to later steps; a later value for the
//! same dof or face replaces the earlier one. Inside one step, repeated
//! `*CLOAD` values for one dof add up.
//!
//! The matrix is the lower triangle of `Tᵀ K T` in compressed column form. The
//! pattern comes from the element connectivity and the constraint transform.
//! It is built once, and a later step with the same constraint structure fills
//! the same pattern again. Element work runs in chunks. Threads compute the
//! element matrices of a chunk, and one thread adds them to the matrix in
//! element order, so the result does not depend on the number of threads.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use fh_inp::{CouplingKind, ElementTarget, Model, MpcKind, Target};

use crate::constraint::{Elimination, Transform};
use crate::element::{self, evaluate, Elastic, ElementState, Evaluation, Kind, Material, MAX_DOFS};
use crate::sparse::{Matrix, NodeGraph};
use crate::SolverError;

/// Results of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub name: Option<String>,
    /// Step time at the end of the step. The steps run one after the other.
    pub time: f64,
    /// Translations, in the order of `Model::nodes`.
    pub displacement: Vec<[f64; 3]>,
    /// Rotations. Zero for nodes without rotational dofs.
    pub rotation: Vec<[f64; 3]>,
    /// Internal force minus applied load, per node. Non-zero at supports.
    pub reaction: Vec<[f64; 3]>,
    /// Volume-averaged stress `[xx, yy, zz, xy, yz, xz]`, in the order of `Model::elements`.
    pub element_stress: Vec<[f64; 6]>,
    pub mises: Vec<f64>,
    /// Stress at the integration points, the points of element 0, then element 1, and so on.
    /// Empty unless `Formats::text` is on, because it is the largest result array.
    pub ip_stress: Vec<[f64; 6]>,
}

/// How the reduced system is solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Cholesky up to `Options::direct_max_dofs` reduced dofs, conjugate gradients above.
    Auto,
    /// Sparse Cholesky (faer). Fast and exact, but the factor can need many times the memory of the matrix.
    Cholesky,
    /// Conjugate gradients with a Jacobi preconditioner. Memory stays near the size of the matrix.
    Pcg,
}

/// The result file formats of a run. All three are on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    /// `step-N.txt`: tables of node results, element results and integration point stress.
    pub text: bool,
    /// `step-N.vtu`: binary VTU.
    pub vtu: bool,
    /// `step-N.vtk`: legacy VTK, ASCII.
    pub vtk: bool,
}

impl Default for Formats {
    fn default() -> Self {
        Formats {
            text: true,
            vtu: true,
            vtk: true,
        }
    }
}

impl Formats {
    /// Read a list such as `text,vtu`. The names are `text`, `vtu` and `vtk`; `all` is the three.
    pub fn parse(list: &str) -> Result<Formats, String> {
        let mut f = Formats {
            text: false,
            vtu: false,
            vtk: false,
        };
        for name in list.split(',').map(str::trim) {
            match name {
                "text" => f.text = true,
                "vtu" => f.vtu = true,
                "vtk" => f.vtk = true,
                "all" => f = Formats::default(),
                other => {
                    return Err(format!(
                        "unknown result format `{other}`: use text, vtu, vtk or all"
                    ))
                }
            }
        }
        Ok(f)
    }
}

/// Settings of the solver.
#[derive(Debug, Clone)]
pub struct Options {
    /// The result files that `run_model` writes.
    pub formats: Formats,
    pub method: Method,
    /// `Auto` uses Cholesky when the reduced system has at most this many dofs.
    pub direct_max_dofs: usize,
    /// Relative residual `|f - K u| / |f|` that the conjugate gradient solver must reach.
    pub tolerance: f64,
    pub max_iterations: usize,
    /// Worker threads for the element work. 0 means all cores of the machine.
    pub threads: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            formats: Formats::default(),
            method: Method::Auto,
            direct_max_dofs: DIRECT_MAX_DOFS,
            tolerance: 1e-10,
            max_iterations: 50_000,
            threads: 0,
        }
    }
}

/// Switch rule of `Method::Auto`. See DESIGN.md for the measurements.
pub const DIRECT_MAX_DOFS: usize = 60_000;

/// A solve fails if the relative residual of the reduced system is above this.
pub const RESIDUAL_LIMIT: f64 = 1e-6;

/// What the solver did in one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepInfo {
    pub free_dofs: usize,
    /// The method that ran (never `Auto`).
    pub method: Method,
    /// Conjugate gradient iterations. 0 for Cholesky.
    pub iterations: usize,
    /// Relative residual of the reduced system after the solve.
    pub residual: f64,
}

/// What a whole run did.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub total_dofs: usize,
    pub steps: Vec<StepInfo>,
    /// How often the matrix pattern was built. A step with the same constraint
    /// structure as the step before reuses the pattern.
    pub pattern_builds: usize,
    /// Entries in the lower triangle of the matrix.
    pub nnz: usize,
    /// Dofs without stiffness that the solver fixed at zero: (node number, dof).
    pub orphans: Vec<(u32, u8)>,
    /// Estimate of the peak memory in bytes.
    pub estimate_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub steps: Vec<StepResult>,
    pub total_dofs: usize,
    /// Dofs left after elimination, per step.
    pub free_dofs: Vec<usize>,
    pub summary: Summary,
}

/// Receives events of a run. All methods have empty defaults.
pub trait Observer {
    /// Estimate of the peak memory, before the big arrays exist.
    fn estimate(&mut self, _bytes: u64) {}
    /// Position in the run: step `step` of `steps` (from 1), a phase name and a fraction of the step.
    fn progress(&mut self, _step: usize, _steps: usize, _phase: &str, _fraction: f64) {}
    /// True stops the run with a cancel error.
    fn cancelled(&mut self) -> bool {
        false
    }
    /// A step is done. The observer owns the result.
    fn step_done(
        &mut self,
        _index: usize,
        _result: StepResult,
        _info: &StepInfo,
    ) -> Result<(), SolverError> {
        Ok(())
    }
}

struct Collect(Vec<StepResult>);

impl Observer for Collect {
    fn step_done(&mut self, _: usize, r: StepResult, _: &StepInfo) -> Result<(), SolverError> {
        self.0.push(r);
        Ok(())
    }
}

/// The time at the end of each step. Steps without a period last 1.0.
pub fn step_times(model: &Model) -> Vec<f64> {
    let mut time = 0.0;
    model
        .steps
        .iter()
        .map(|s| {
            time += s
                .static_procedure
                .as_ref()
                .and_then(|p| p.time_period)
                .unwrap_or(1.0);
            time
        })
        .collect()
}

/// The error text of a run that the observer cancelled.
pub const CANCELLED: &str = "cancelled";

/// Solve every step of the model with the default options.
pub fn solve(model: &Model) -> Result<Solution, SolverError> {
    solve_options(model, &Options::default())
}

/// Solve every step of the model and keep all results.
pub fn solve_options(model: &Model, options: &Options) -> Result<Solution, SolverError> {
    let mut collect = Collect(Vec::new());
    let summary = solve_with(model, options, &mut collect)?;
    Ok(Solution {
        steps: collect.0,
        total_dofs: summary.total_dofs,
        free_dofs: summary.steps.iter().map(|s| s.free_dofs).collect(),
        summary,
    })
}

/// Solve every step. Each result goes to `observer` as soon as the step is done.
pub fn solve_with(
    model: &Model,
    options: &Options,
    observer: &mut dyn Observer,
) -> Result<Summary, SolverError> {
    solve_from(model, options, observer, 0)
}

/// Like [`solve_with`], but the first `skip` steps only add their data and are not solved.
/// A restart uses this: the loads and constraints of the skipped steps still carry over.
pub fn solve_from(
    model: &Model,
    options: &Options,
    observer: &mut dyn Observer,
    skip: usize,
) -> Result<Summary, SolverError> {
    if model.steps.is_empty() {
        return Err(SolverError::new("the model has no *STEP"));
    }
    if model.steps.iter().any(|s| s.nlgeom) {
        return Err(SolverError::new("NLGEOM=YES is not available before M7"));
    }
    let threads = match options.threads {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()),
        n => n,
    };
    let mesh = Mesh::new(model)?;
    let graph = NodeGraph::new(model.nodes.len(), &mesh.conn, &mesh.conn_start);
    let mut summary = Summary {
        total_dofs: mesh.n_dofs,
        steps: Vec::new(),
        pattern_builds: 0,
        nnz: 0,
        orphans: Vec::new(),
        estimate_bytes: 0,
    };
    // The matrix has about 9 entries for each pair of neighbour nodes, half of them in the lower triangle.
    let nnz_guess = (9 * graph.entries() + 3 * model.nodes.len()) / 2;
    let method_guess = options.method != Method::Cholesky && mesh.n_dofs > options.direct_max_dofs;
    summary.estimate_bytes = estimate_bytes(&Sizes {
        nodes: model.nodes.len(),
        elements: model.elements.len(),
        dofs: mesh.n_dofs,
        nnz: nnz_guess,
        graph_bytes: graph.bytes(),
        threads,
        iterative: method_guess || options.method == Method::Pcg,
        ip_stress: options.formats.text,
    });
    observer.estimate(summary.estimate_bytes);

    let mut bc: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut loads: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut pressures: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    mesh.add_bcs(&model.boundaries, &mut bc)?;
    let mut cache: Option<(Transform, Matrix)> = None;
    let mut orphans: BTreeSet<(u32, u8)> = BTreeSet::new();
    let times = step_times(model);
    let mut time;
    let n_steps = model.steps.len();
    for (index, step) in model.steps.iter().enumerate() {
        mesh.add_bcs(&step.boundaries, &mut bc)?;
        // Loads on one dof add up inside a step. A later step replaces the value.
        let mut step_loads: BTreeMap<(usize, u8), f64> = BTreeMap::new();
        for c in &step.cloads {
            for n in mesh.nodes_of(&c.target)? {
                mesh.check_dof(n, c.dof)?;
                *step_loads.entry((n, c.dof)).or_insert(0.0) += c.magnitude;
            }
        }
        loads.extend(step_loads);
        for dl in &step.dloads {
            for e in mesh.elements_of(&dl.target)? {
                pressures.insert((e, dl.face), dl.pressure);
            }
        }
        time = times[index];
        if index < skip {
            continue;
        }
        if observer.cancelled() {
            return Err(SolverError::new(CANCELLED));
        }
        let ctx = StepContext {
            mesh: &mesh,
            graph: &graph,
            options,
            threads,
            index: index + 1,
            n_steps,
        };
        let (result, info) = ctx.solve_step(
            observer,
            step.name.clone(),
            time,
            &bc,
            &loads,
            &pressures,
            &mut cache,
            &mut summary.pattern_builds,
            &mut summary.nnz,
            &mut orphans,
        )?;
        observer.step_done(index, result, &info)?;
        summary.steps.push(info);
    }
    summary.orphans = orphans.into_iter().collect();
    Ok(summary)
}

/// Sizes that decide the memory of a run.
#[derive(Debug, Clone, Copy)]
pub struct Sizes {
    pub nodes: usize,
    pub elements: usize,
    pub dofs: usize,
    /// Entries in the lower triangle of the matrix.
    pub nnz: usize,
    pub graph_bytes: usize,
    pub threads: usize,
    /// True for the conjugate gradient solver.
    pub iterative: bool,
    /// True when the stress at the integration points is kept for the text file.
    pub ip_stress: bool,
}

/// Estimate of the peak memory of a run in bytes, without the Cholesky factor.
/// The formula is in DESIGN.md. The input model is part of it.
pub fn estimate_bytes(s: &Sizes) -> u64 {
    let (nodes, els, n, nnz) = (
        s.nodes as u64,
        s.elements as u64,
        s.dofs as u64,
        s.nnz as u64,
    );
    // The input model: nodes, elements with their node lists, and the sets.
    let model = 40 * nodes + 130 * els;
    // The mesh: connectivity, element tables, two id maps and the node graph.
    let mesh = 45 * els + 30 * (nodes + els) + s.graph_bytes as u64 + 8 * nodes;
    // The matrix: 32-bit column pointers and rows, and the values.
    let matrix = 12 * nnz + 4 * n;
    // The transform, and the full and reduced vectors of the step.
    let vectors = 24 * n + 48 * n + if s.iterative { 72 * n } else { 24 * n };
    // The Cholesky factor of a 3-D mesh: fill grows as n^1.35 (measured, DESIGN.md section 11).
    let factor = if s.iterative {
        0
    } else {
        (400.0 * (n as f64).powf(1.35)) as u64
    };
    // The results of one step and the output grid.
    let results =
        72 * nodes + 56 * els + 120 * nodes + 70 * els + if s.ip_stress { 8 * 48 * els } else { 0 };
    // The chunk buffers of the threads.
    let chunks = s.threads as u64 * 256 * std::mem::size_of::<Evaluation>() as u64;
    // Allocator slack, the input text, and parts the terms miss. The measured peak is 1.21 to
    // 1.26 times the sum (iterative path, 1M elements: 1.26). The factor 1.35 keeps the
    // estimate above the peak (DESIGN.md section 11).
    let sum = model + mesh + matrix + vectors + factor + results + chunks;
    sum + sum * 7 / 20
}

struct Mesh<'a> {
    model: &'a Model,
    node_index: HashMap<u32, usize>,
    elem_index: HashMap<u32, usize>,
    /// First dof of each node.
    base: Vec<usize>,
    n_dofs: usize,
    kinds: Vec<Kind>,
    /// Index into `mats` for each element.
    mat: Vec<u16>,
    mats: Vec<Elastic>,
    /// Node indices of all elements, one after the other.
    conn: Vec<u32>,
    /// Position in `conn` of the first node of each element, and one past the end.
    conn_start: Vec<u32>,
}

impl<'a> Mesh<'a> {
    fn new(model: &'a Model) -> Result<Self, SolverError> {
        let node_index: HashMap<u32, usize> = model
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        let elem_index: HashMap<u32, usize> = model
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let node = |id: u32| {
            node_index
                .get(&id)
                .copied()
                .ok_or_else(|| SolverError::new(format!("node {id} not found")))
        };
        // Nodes that carry rotations: reference nodes and rigid beam ends.
        let mut rot = vec![false; model.nodes.len()];
        for c in &model.couplings {
            if c.kind == CouplingKind::Kinematic || c.last_dof >= 4 {
                rot[node(c.ref_node)?] = true;
            }
        }
        for m in model.mpcs.iter().filter(|m| m.kind == MpcKind::Beam) {
            for n in &m.nodes {
                rot[node(*n)?] = true;
            }
        }
        let mut base = Vec::with_capacity(rot.len());
        let mut n_dofs = 0;
        for r in &rot {
            base.push(n_dofs);
            n_dofs += if *r { 6 } else { 3 };
        }
        if n_dofs >= u32::MAX as usize {
            return Err(SolverError::new(format!(
                "the model has {n_dofs} dofs, more than the 32-bit index allows"
            )));
        }
        // Material of each element, from the sections.
        let mut material: HashMap<u32, &str> = HashMap::new();
        for s in &model.sections {
            let set = model
                .elset(&s.elset)
                .ok_or_else(|| SolverError::new(format!("element set {} not found", s.elset)))?;
            for id in &set.ids {
                material.insert(*id, &s.material);
            }
        }
        let mut mat_index: HashMap<&str, u16> = HashMap::new();
        let mut mats = Vec::new();
        let n_el = model.elements.len();
        let (mut kinds, mut mat, mut conn) = (
            Vec::with_capacity(n_el),
            Vec::with_capacity(n_el),
            Vec::with_capacity(8 * n_el),
        );
        let mut conn_start = Vec::with_capacity(n_el + 1);
        for el in &model.elements {
            let kind = Kind::parse(&el.element_type).ok_or_else(|| {
                SolverError::new(format!(
                    "element {}: type {} is not available yet",
                    el.id, el.element_type
                ))
            })?;
            let name = material.get(&el.id).ok_or_else(|| {
                SolverError::new(format!("element {} has no *SOLID SECTION", el.id))
            })?;
            let index = match mat_index.get(name) {
                Some(&i) => i,
                None => {
                    let m = model
                        .material(name)
                        .and_then(|m| m.elastic.as_ref())
                        .ok_or_else(|| {
                            SolverError::new(format!("material {name} has no *ELASTIC"))
                        })?;
                    let i = u16::try_from(mats.len())
                        .map_err(|_| SolverError::new("more than 65535 materials"))?;
                    mats.push(Elastic::new(m.youngs_modulus, m.poissons_ratio));
                    mat_index.insert(name, i);
                    i
                }
            };
            if el.nodes.len() != kind.node_count() {
                return Err(SolverError::new(format!(
                    "element {}: {} needs {} nodes, found {}",
                    el.id,
                    el.element_type,
                    kind.node_count(),
                    el.nodes.len()
                )));
            }
            kinds.push(kind);
            mat.push(index);
            conn_start.push(conn.len() as u32);
            for n in &el.nodes {
                conn.push(node(*n)? as u32);
            }
        }
        conn_start.push(conn.len() as u32);
        Ok(Mesh {
            model,
            node_index,
            elem_index,
            base,
            n_dofs,
            kinds,
            mat,
            mats,
            conn,
            conn_start,
        })
    }

    fn dof(&self, node: usize, d: u8) -> usize {
        self.base[node] + usize::from(d) - 1
    }

    fn node_dofs(&self, node: usize) -> usize {
        if node + 1 < self.base.len() {
            self.base[node + 1] - self.base[node]
        } else {
            self.n_dofs - self.base[node]
        }
    }

    fn check_dof(&self, node: usize, d: u8) -> Result<(), SolverError> {
        if usize::from(d) > self.node_dofs(node) {
            return Err(SolverError::new(format!(
                "node {} has no degree of freedom {d}",
                self.model.nodes[node].id
            )));
        }
        Ok(())
    }

    fn node(&self, id: u32) -> Result<usize, SolverError> {
        self.node_index
            .get(&id)
            .copied()
            .ok_or_else(|| SolverError::new(format!("node {id} not found")))
    }

    fn nodes_of(&self, t: &Target) -> Result<Vec<usize>, SolverError> {
        match t {
            Target::Node(id) => Ok(vec![self.node(*id)?]),
            Target::Set(name) => {
                let set = self
                    .model
                    .nset(name)
                    .ok_or_else(|| SolverError::new(format!("node set {name} not found")))?;
                set.ids.iter().map(|id| self.node(*id)).collect()
            }
        }
    }

    fn element(&self, id: u32) -> Result<usize, SolverError> {
        self.elem_index
            .get(&id)
            .copied()
            .ok_or_else(|| SolverError::new(format!("element {id} not found")))
    }

    fn elements_of(&self, t: &ElementTarget) -> Result<Vec<usize>, SolverError> {
        match t {
            ElementTarget::Element(id) => Ok(vec![self.element(*id)?]),
            ElementTarget::Set(name) => {
                let set = self
                    .model
                    .elset(name)
                    .ok_or_else(|| SolverError::new(format!("element set {name} not found")))?;
                set.ids.iter().map(|id| self.element(*id)).collect()
            }
        }
    }

    fn add_bcs(
        &self,
        list: &[fh_inp::Boundary],
        bc: &mut BTreeMap<(usize, u8), f64>,
    ) -> Result<(), SolverError> {
        for b in list {
            for n in self.nodes_of(&b.target)? {
                for d in b.first_dof..=b.last_dof {
                    self.check_dof(n, d)?;
                    bc.insert((n, d), b.value);
                }
            }
        }
        Ok(())
    }

    fn surface_nodes(&self, name: &str, ref_node: usize) -> Result<Vec<usize>, SolverError> {
        let s = self
            .model
            .surfaces
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| SolverError::new(format!("surface {name} not found")))?;
        let mut v: Vec<usize> = s
            .ids
            .iter()
            .map(|id| self.node(*id))
            .collect::<Result<_, _>>()?;
        v.sort_unstable();
        v.dedup();
        v.retain(|&n| n != ref_node);
        Ok(v)
    }

    fn xyz(&self, node: usize) -> [f64; 3] {
        self.model.nodes[node].xyz
    }

    /// Prescribed values first, then the model constraints.
    fn elimination(&self, bc: &BTreeMap<(usize, u8), f64>) -> Result<Elimination, SolverError> {
        let mut el = Elimination::new(self.n_dofs);
        for (&(n, d), &v) in bc {
            let dof = self.dof(n, d);
            el.add(&[(dof, 1.0)], v, Some(dof))
                .map_err(|e| SolverError::new(format!("node {}: {e}", self.model.nodes[n].id)))?;
        }
        for eq in &self.model.equations {
            let row: Vec<(usize, f64)> = eq
                .terms
                .iter()
                .map(|t| {
                    let n = self.node(t.node)?;
                    self.check_dof(n, t.dof)?;
                    Ok((self.dof(n, t.dof), t.coefficient))
                })
                .collect::<Result<_, SolverError>>()?;
            el.add(&row, 0.0, None)?;
        }
        for m in &self.model.mpcs {
            self.add_mpc(&mut el, m)?;
        }
        for c in &self.model.couplings {
            self.add_coupling(&mut el, c)?;
        }
        Ok(el)
    }

    fn add_mpc(&self, el: &mut Elimination, m: &fh_inp::Mpc) -> Result<(), SolverError> {
        let (a, b) = (self.node(m.nodes[0])?, self.node(m.nodes[1])?);
        let r = sub(self.xyz(b), self.xyz(a));
        match m.kind {
            MpcKind::Tie | MpcKind::Pin => {
                for c in 1..=3 {
                    el.add(
                        &[(self.dof(b, c), 1.0), (self.dof(a, c), -1.0)],
                        0.0,
                        Some(self.dof(b, c)),
                    )?;
                }
            }
            MpcKind::Link => {
                let len = norm(r);
                if len == 0.0 {
                    return Err(SolverError::new("*MPC LINK joins two nodes at one point"));
                }
                let mut row = Vec::new();
                for (c, rc) in r.iter().enumerate() {
                    row.push((self.dof(b, c as u8 + 1), rc / len));
                    row.push((self.dof(a, c as u8 + 1), -rc / len));
                }
                el.add(&row, 0.0, None)?;
            }
            MpcKind::Beam => {
                for c in 0..3 {
                    let mut row = vec![
                        (self.dof(b, c as u8 + 1), 1.0),
                        (self.dof(a, c as u8 + 1), -1.0),
                    ];
                    row.extend(self.rot_terms(a, r, c));
                    el.add(&row, 0.0, Some(self.dof(b, c as u8 + 1)))?;
                    el.add(
                        &[
                            (self.dof(b, c as u8 + 4), 1.0),
                            (self.dof(a, c as u8 + 4), -1.0),
                        ],
                        0.0,
                        Some(self.dof(b, c as u8 + 4)),
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Terms of `-(theta x r)_c` for the rotation dofs of `node`.
    fn rot_terms(&self, node: usize, r: [f64; 3], c: usize) -> Vec<(usize, f64)> {
        // (theta x r)_c = theta_{c+1} r_{c+2} - theta_{c+2} r_{c+1}
        let (p, q) = ((c + 1) % 3, (c + 2) % 3);
        vec![
            (self.dof(node, p as u8 + 4), -r[q]),
            (self.dof(node, q as u8 + 4), r[p]),
        ]
    }

    fn add_coupling(&self, el: &mut Elimination, c: &fh_inp::Coupling) -> Result<(), SolverError> {
        let rn = self.node(c.ref_node)?;
        let slaves = self.surface_nodes(&c.surface, rn)?;
        if slaves.is_empty() {
            return Err(SolverError::new(format!(
                "coupling {} has no slave nodes",
                c.name
            )));
        }
        let in_range = |d: u8| (c.first_dof..=c.last_dof).contains(&d);
        match c.kind {
            CouplingKind::Kinematic => {
                for &s in &slaves {
                    let r = sub(self.xyz(s), self.xyz(rn));
                    for k in 0..3 {
                        if !in_range(k as u8 + 1) {
                            continue;
                        }
                        let mut row = vec![
                            (self.dof(s, k as u8 + 1), 1.0),
                            (self.dof(rn, k as u8 + 1), -1.0),
                        ];
                        row.extend(self.rot_terms(rn, r, k));
                        el.add(&row, 0.0, Some(self.dof(s, k as u8 + 1)))?;
                    }
                }
            }
            CouplingKind::Distributing => self.distributing(el, c, rn, &slaves)?,
        }
        Ok(())
    }

    /// The reference node follows the average motion of the slaves and their
    /// least-squares rigid rotation `theta`: `u_ref = mean(u) + theta x (x_ref - centre)`.
    /// The rotation term carries the moment of a load at an off-centre
    /// reference node to the slaves. If the reference has rotation dofs in the
    /// range, they follow `theta`.
    fn distributing(
        &self,
        el: &mut Elimination,
        c: &fh_inp::Coupling,
        rn: usize,
        slaves: &[usize],
    ) -> Result<(), SolverError> {
        let w = 1.0 / slaves.len() as f64;
        let mut centre = [0.0; 3];
        for &s in slaves {
            for (c, x) in centre.iter_mut().zip(self.xyz(s)) {
                *c += w * x;
            }
        }
        let arm = sub(self.xyz(rn), centre);
        let want_rotation = c.last_dof >= 4;
        let needs_fit = want_rotation || arm.iter().any(|v| v.abs() > 1e-12);
        let fit = if needs_fit {
            Some(self.rotation_fit(c, slaves, centre, w)?)
        } else {
            None
        };
        let in_range = |d: u8| (c.first_dof..=c.last_dof).contains(&d);
        for k in 0..3 {
            if !in_range(k as u8 + 1) {
                continue;
            }
            let mut terms: BTreeMap<usize, f64> = BTreeMap::new();
            for &s in slaves {
                *terms.entry(self.dof(s, k as u8 + 1)).or_insert(0.0) += w;
            }
            if let Some(fit) = &fit {
                // (theta x arm)_k = theta_p arm_q - theta_q arm_p.
                let (p, q) = ((k + 1) % 3, (k + 2) % 3);
                for (dof, coef) in &fit[p] {
                    *terms.entry(*dof).or_insert(0.0) += arm[q] * coef;
                }
                for (dof, coef) in &fit[q] {
                    *terms.entry(*dof).or_insert(0.0) -= arm[p] * coef;
                }
            }
            let mut row = vec![(self.dof(rn, k as u8 + 1), 1.0)];
            row.extend(terms.into_iter().map(|(d, v)| (d, -v)));
            el.add(&row, 0.0, Some(self.dof(rn, k as u8 + 1)))?;
        }
        if let Some(fit) = &fit {
            for k in 0..3u8 {
                if !in_range(k + 4) {
                    continue;
                }
                let mut row = vec![(self.dof(rn, k + 4), 1.0)];
                row.extend(fit[usize::from(k)].iter().map(|(&d, &v)| (d, -v)));
                el.add(&row, 0.0, Some(self.dof(rn, k + 4)))?;
            }
        }
        Ok(())
    }

    /// For each rotation component `theta_k`, the coefficients of the slave
    /// translation dofs: `theta = A^-1 * sum(w r x u)`.
    fn rotation_fit(
        &self,
        c: &fh_inp::Coupling,
        slaves: &[usize],
        centre: [f64; 3],
        w: f64,
    ) -> Result<[BTreeMap<usize, f64>; 3], SolverError> {
        let mut a = [[0.0; 3]; 3];
        for &s in slaves {
            let r = sub(self.xyz(s), centre);
            for i in 0..3 {
                for j in 0..3 {
                    a[i][j] += w * (if i == j { dot(r, r) } else { 0.0 } - r[i] * r[j]);
                }
            }
        }
        let scale = a.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()));
        let (_, det) = crate::dense::inv3(&a);
        let inv = crate::dense::invert(&a)
            .filter(|_| det.abs() > 1e-12 * scale.powi(3))
            .ok_or_else(|| {
                SolverError::new(format!(
                    "coupling {}: slave nodes are on a line, rotation is undefined",
                    c.name
                ))
            })?;
        let mut fit: [BTreeMap<usize, f64>; 3] = Default::default();
        for (k, terms) in fit.iter_mut().enumerate() {
            // (r x u)_m = eps_mnp r_n u_p.
            for &s in slaves {
                let r = sub(self.xyz(s), centre);
                let rx = [[0.0, -r[2], r[1]], [r[2], 0.0, -r[0]], [-r[1], r[0], 0.0]];
                for p in 0..3 {
                    let coef: f64 = rx
                        .iter()
                        .enumerate()
                        .map(|(m, row)| inv[k][m] * row[p])
                        .sum::<f64>()
                        * w;
                    *terms.entry(self.dof(s, p as u8 + 1)).or_insert(0.0) += coef;
                }
            }
        }
        Ok(fit)
    }

    /// Node indices and dofs of element `e`. Returns the dof count.
    fn element_dofs(&self, e: usize, dofs: &mut [usize; MAX_DOFS]) -> usize {
        let nodes = &self.conn[self.conn_start[e] as usize..self.conn_start[e + 1] as usize];
        for (a, &n) in nodes.iter().enumerate() {
            for c in 0..3 {
                dofs[3 * a + c] = self.base[n as usize] + c;
            }
        }
        3 * nodes.len()
    }

    fn element_coords(&self, e: usize, coords: &mut [[f64; 3]; 10]) -> usize {
        let nodes = &self.conn[self.conn_start[e] as usize..self.conn_start[e + 1] as usize];
        for (c, &n) in coords.iter_mut().zip(nodes) {
            *c = self.model.nodes[n as usize].xyz;
        }
        nodes.len()
    }

    fn element_error(&self, e: usize, err: SolverError) -> SolverError {
        SolverError::new(format!("element {}: {err}", self.model.elements[e].id))
    }

    /// Bytes that the tables of the mesh and the node graph hold, for the size checks.
    fn evaluate_element(
        &self,
        e: usize,
        u: &[f64],
        state: &mut ElementState,
        out: &mut Evaluation,
        tangent: bool,
    ) -> Result<(), SolverError> {
        let mut coords = [[0.0; 3]; 10];
        let n = self.element_coords(e, &mut coords);
        let material: &dyn Material = &self.mats[usize::from(self.mat[e])];
        evaluate(
            self.kinds[e],
            &coords[..n],
            material,
            &u[..3 * n],
            state,
            out,
            tangent,
        )
        .map_err(|err| self.element_error(e, err))
    }
}

struct StepContext<'a> {
    mesh: &'a Mesh<'a>,
    graph: &'a NodeGraph,
    options: &'a Options,
    threads: usize,
    index: usize,
    n_steps: usize,
}

/// Elements per thread in one chunk.
const CHUNK: usize = 256;

impl StepContext<'_> {
    /// Run `work` for the elements `0..n`. Threads compute chunks in parallel,
    /// and `consume` sees the results in element order on this thread.
    fn for_elements(
        &self,
        n: usize,
        work: &(dyn Fn(usize, &mut Evaluation) -> Result<(), SolverError> + Sync),
        consume: &mut dyn FnMut(usize, &Evaluation),
        tick: &mut dyn FnMut(f64) -> Result<(), SolverError>,
    ) -> Result<(), SolverError> {
        let threads = self.threads.max(1);
        let chunk = threads * CHUNK;
        let mut buf = vec![Evaluation::default(); chunk.min(n.max(1))];
        let mut start = 0;
        while start < n {
            let len = chunk.min(n - start);
            if threads == 1 || len < 2 * threads {
                for (i, ev) in buf[..len].iter_mut().enumerate() {
                    work(start + i, ev)?;
                }
            } else {
                let per = len.div_ceil(threads);
                let mut results: Vec<Result<(), SolverError>> = Vec::new();
                std::thread::scope(|scope| {
                    let handles: Vec<_> = buf[..len]
                        .chunks_mut(per)
                        .enumerate()
                        .map(|(t, part)| {
                            scope.spawn(move || {
                                for (i, ev) in part.iter_mut().enumerate() {
                                    work(start + t * per + i, ev)?;
                                }
                                Ok(())
                            })
                        })
                        .collect();
                    for h in handles {
                        results.push(h.join().unwrap_or_else(|_| {
                            Err(SolverError::new("an element thread stopped unexpectedly"))
                        }));
                    }
                });
                // The first error in element order wins.
                for r in results {
                    r?;
                }
            }
            for (i, ev) in buf[..len].iter().enumerate() {
                consume(start + i, ev);
            }
            start += len;
            tick(start as f64 / n as f64)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn solve_step(
        &self,
        observer: &mut dyn Observer,
        name: Option<String>,
        time: f64,
        bc: &BTreeMap<(usize, u8), f64>,
        loads: &BTreeMap<(usize, u8), f64>,
        pressures: &BTreeMap<(usize, u8), f64>,
        cache: &mut Option<(Transform, Matrix)>,
        pattern_builds: &mut usize,
        nnz: &mut usize,
        orphans: &mut BTreeSet<(u32, u8)>,
    ) -> Result<(StepResult, StepInfo), SolverError> {
        let mesh = self.mesh;
        let (index, n_steps) = (self.index, self.n_steps);
        let mut f = vec![0.0; mesh.n_dofs];
        for (&(n, d), &v) in loads {
            f[mesh.dof(n, d)] += v;
        }
        for (&(e, face), &p) in pressures {
            let mut coords = [[0.0; 3]; 10];
            let n = mesh.element_coords(e, &mut coords);
            for (local, force) in element::pressure_load(mesh.kinds[e], &coords[..n], face, p)
                .map_err(|err| mesh.element_error(e, err))?
            {
                let node = mesh.conn[mesh.conn_start[e] as usize + local] as usize;
                for (k, v) in force.iter().enumerate() {
                    f[mesh.dof(node, k as u8 + 1)] += v;
                }
            }
        }
        // Constraints. A dof that no element reaches has no stiffness. It is fixed at zero.
        let mut el = mesh.elimination(bc)?;
        let mut t = el.finish();
        let fixed = self.orphan_dofs(&t);
        if !fixed.is_empty() {
            for &d in &fixed {
                let node = mesh.base.partition_point(|&b| b <= d) - 1;
                let (id, comp) = (mesh.model.nodes[node].id, (d - mesh.base[node]) as u8 + 1);
                if f[d] != 0.0 {
                    return Err(SolverError::new(format!(
                        "node {id} has a load on dof {comp} but no element gives it stiffness"
                    )));
                }
                orphans.insert((id, comp));
                el.add(&[(d, 1.0)], 0.0, Some(d))?;
            }
            t = el.finish();
        }
        observer.progress(index, n_steps, "assemble", 0.0);
        let reuse = cache
            .as_ref()
            .is_some_and(|(old, _)| old.same_structure(&t));
        if reuse {
            if let Some((old, m)) = cache.as_mut() {
                *old = t.clone();
                m.clear();
            }
        } else {
            *cache = None;
            let base = &mesh.base;
            let m = Matrix::pattern(self.graph, &|n, c| base[n] + c, &t)?;
            *pattern_builds += 1;
            *cache = Some((t.clone(), m));
        }
        let Some((_, matrix)) = cache.as_mut() else {
            return Err(SolverError::new("matrix pattern missing"));
        };
        *nnz = matrix.nnz();
        // Assemble Tᵀ K T and the right side Tᵀ (f - K u0).
        let mut rhs_full = f.clone();
        let zeros = [0.0; MAX_DOFS];
        {
            let work = |e: usize, ev: &mut Evaluation| {
                mesh.evaluate_element(e, &zeros, &mut ElementState::default(), ev, true)
            };
            let mut consume = |e: usize, ev: &Evaluation| {
                let mut dofs = [0; MAX_DOFS];
                let n = mesh.element_dofs(e, &mut dofs);
                let u0: Vec<f64> = dofs[..n].iter().map(|&d| t.offset[d]).collect();
                let shifted = u0.iter().any(|v| *v != 0.0);
                for i in 0..n {
                    if shifted {
                        rhs_full[dofs[i]] -= (0..n).map(|j| ev.k[i][j] * u0[j]).sum::<f64>();
                    }
                    let (ca, ka) = t.row(dofs[i]);
                    for (j, &dj) in dofs[..n].iter().enumerate() {
                        let v = ev.k[i][j];
                        if v == 0.0 {
                            continue;
                        }
                        let (cb, kb) = t.row(dj);
                        for (&a, &xa) in ca.iter().zip(ka) {
                            for (&b, &xb) in cb.iter().zip(kb) {
                                if a >= b {
                                    matrix.add(a, b, xa * xb * v);
                                }
                            }
                        }
                    }
                }
            };
            let mut tick = |fraction: f64| {
                observer.progress(index, n_steps, "assemble", fraction);
                if observer.cancelled() {
                    Err(SolverError::new(CANCELLED))
                } else {
                    Ok(())
                }
            };
            self.for_elements(mesh.kinds.len(), &work, &mut consume, &mut tick)?;
        }
        // Solve.
        let fr = t.reduce(&rhs_full);
        drop(rhs_full);
        let method = match self.options.method {
            Method::Auto if t.n_free <= self.options.direct_max_dofs => Method::Cholesky,
            Method::Auto => Method::Pcg,
            m => m,
        };
        let mut iterations = 0;
        let mut ur = vec![0.0; t.n_free];
        if t.n_free > 0 {
            observer.progress(index, n_steps, "solve", 0.0);
            ur = match method {
                Method::Pcg => {
                    let tol = self.options.tolerance;
                    let mut stop = false;
                    let solved =
                        matrix.pcg_solve(&fr, tol, self.options.max_iterations, &mut |_, rel| {
                            let done = (1.0 / rel.max(1e-300)).ln() / (1.0 / tol).ln();
                            observer.progress(index, n_steps, "solve", done.clamp(0.0, 0.99));
                            stop = observer.cancelled();
                            !stop
                        });
                    match solved {
                        Ok((x, its)) => {
                            iterations = its;
                            x
                        }
                        Err(_) if stop => return Err(SolverError::new(CANCELLED)),
                        Err(e) => return Err(e),
                    }
                }
                _ => matrix.cholesky_solve(&fr)?,
            };
        }
        // Residual of the reduced system.
        let residual = {
            let mut kx = vec![0.0; t.n_free];
            matrix.mul(&ur, &mut kx);
            let num = kx
                .iter()
                .zip(&fr)
                .map(|(a, b)| (b - a) * (b - a))
                .sum::<f64>();
            let den = fr.iter().map(|b| b * b).sum::<f64>();
            if den > 0.0 {
                (num / den).sqrt()
            } else {
                num.sqrt()
            }
        };
        if residual.is_nan() || residual > RESIDUAL_LIMIT {
            return Err(SolverError::new(format!(
                "the solution does not satisfy the equations: relative residual {residual:e}, limit {RESIDUAL_LIMIT:e}. Check the boundary conditions and the constraints"
            )));
        }
        drop(fr);
        let u = t.expand(&ur);
        drop(ur);
        // Reactions and stress from the element forces.
        observer.progress(index, n_steps, "recover", 0.0);
        let mut fint = vec![0.0; mesh.n_dofs];
        let mut stress = vec![[0.0; 6]; mesh.kinds.len()];
        let keep_ip = self.options.formats.text;
        let mut ip_stress: Vec<[f64; 6]> = Vec::new();
        {
            let work = |e: usize, ev: &mut Evaluation| {
                let mut dofs = [0; MAX_DOFS];
                let n = mesh.element_dofs(e, &mut dofs);
                let mut ue = [0.0; MAX_DOFS];
                for (v, &d) in ue.iter_mut().zip(&dofs[..n]) {
                    *v = u[d];
                }
                mesh.evaluate_element(e, &ue, &mut ElementState::default(), ev, false)
            };
            let mut consume = |e: usize, ev: &Evaluation| {
                let mut dofs = [0; MAX_DOFS];
                let n = mesh.element_dofs(e, &mut dofs);
                for (&d, v) in dofs[..n].iter().zip(&ev.fint) {
                    fint[d] += v;
                }
                stress[e] = ev.mean_stress;
                if keep_ip {
                    ip_stress.extend_from_slice(&ev.ip_stress[..mesh.kinds[e].point_count()]);
                }
            };
            let mut tick = |fraction: f64| {
                observer.progress(index, n_steps, "recover", fraction);
                if observer.cancelled() {
                    Err(SolverError::new(CANCELLED))
                } else {
                    Ok(())
                }
            };
            self.for_elements(mesh.kinds.len(), &work, &mut consume, &mut tick)?;
        }
        let n_nodes = mesh.model.nodes.len();
        let grab = |v: &[f64], first: u8| -> Vec<[f64; 3]> {
            (0..n_nodes)
                .map(|n| {
                    let mut o = [0.0; 3];
                    for (k, x) in o.iter_mut().enumerate() {
                        if usize::from(first) + k <= mesh.node_dofs(n) {
                            *x = v[mesh.dof(n, first + k as u8)];
                        }
                    }
                    o
                })
                .collect()
        };
        for (r, fx) in fint.iter_mut().zip(&f) {
            *r -= fx;
        }
        let result = StepResult {
            name,
            time,
            displacement: grab(&u, 1),
            rotation: grab(&u, 4),
            reaction: grab(&fint, 1),
            mises: stress.iter().map(element::von_mises).collect(),
            element_stress: stress,
            ip_stress,
        };
        let info = StepInfo {
            free_dofs: t.n_free,
            method,
            iterations,
            residual,
        };
        observer.progress(index, n_steps, "recover", 1.0);
        Ok((result, info))
    }

    /// Full dofs that stay free after elimination but that no element stiffness reaches.
    fn orphan_dofs(&self, t: &Transform) -> Vec<usize> {
        let mesh = self.mesh;
        let mut used = vec![false; t.n_free];
        for n in 0..mesh.model.nodes.len() {
            if self.graph.neighbours(n).is_empty() {
                continue;
            }
            for c in 0..3 {
                for &r in t.row(mesh.base[n] + c).0 {
                    used[r as usize] = true;
                }
            }
        }
        (0..t.n_free)
            .filter(|&r| !used[r])
            .map(|r| t.full_dof(r))
            .collect()
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}
