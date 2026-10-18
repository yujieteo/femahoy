//! Public types of a run: options, results, the observer, and the entry points.
//!
//! A step is a list of increments. A linear step (every material linear) is one increment
//! that goes through the same loop: it builds `K` once, factors it once, and reuses the
//! factor in later steps. A nonlinear step runs the Newton loop in `step.rs`.
//! Boundary conditions, concentrated loads and pressure loads carry over to later steps.
//! A later value for the same dof or face replaces the earlier one. Inside one step,
//! repeated `*CLOAD` values for one dof add up.

use std::fmt;
use std::sync::Arc;

use fh_inp::Model;

use crate::contribution::ContributionFactory;
use crate::element::{Elastic, Evaluation, Material};
use crate::mesh::Mesh;
use crate::sparse::{NodeGraph, PcgSolver};
use crate::state::{StateImage, StateStore};
use crate::SolverError;

/// Results of one increment (the end of a step for a linear step).
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub name: Option<String>,
    /// Step time at the end of the increment. The steps run one after the other.
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

/// The result file formats of a run. Only VTU (with PVD) is on by default.
/// Decision D-04, see DESIGN.md.
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
    /// VTU with the PVD collection only. Text and legacy VTK are large, so they are options.
    fn default() -> Self {
        Formats {
            text: false,
            vtu: true,
            vtk: false,
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
                "all" => {
                    f = Formats {
                        text: true,
                        vtu: true,
                        vtk: true,
                    }
                }
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

type Build = dyn Fn(&fh_inp::Material) -> Result<Box<dyn Material>, SolverError> + Send + Sync;

/// Builds a material by name from the data of the model. The default factory builds linear
/// elasticity from `*ELASTIC`. A later milestone adds plasticity here. Element code never
/// reads material constants.
#[derive(Clone)]
pub struct MaterialFactory(Arc<Build>);

impl MaterialFactory {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&fh_inp::Material) -> Result<Box<dyn Material>, SolverError> + Send + Sync + 'static,
    {
        MaterialFactory(Arc::new(f))
    }

    pub fn build(&self, m: &fh_inp::Material) -> Result<Box<dyn Material>, SolverError> {
        (self.0)(m)
    }

    /// Linear elasticity from `*ELASTIC`.
    pub fn elastic() -> Self {
        MaterialFactory::new(|m| {
            let e = m
                .elastic
                .as_ref()
                .ok_or_else(|| SolverError::new(format!("material {} has no *ELASTIC", m.name)))?;
            // A model from code does not go through the checks of the parser.
            let (ym, nu) = (e.youngs_modulus, e.poissons_ratio);
            if !(ym > 0.0 && ym.is_finite() && nu > -1.0 && nu < 0.5) {
                return Err(SolverError::new(format!(
                    "material {}: *ELASTIC needs a finite positive Young's modulus and a \
                     Poisson's ratio in (-1, 0.5), found {ym} and {nu}",
                    m.name
                )));
            }
            let elastic = Elastic::new(ym, nu);
            if elastic.d.iter().flatten().any(|v| !v.is_finite()) {
                return Err(SolverError::new(format!(
                    "material {}: the elasticity matrix for {ym} and {nu} is not finite",
                    m.name
                )));
            }
            Ok(Box::new(elastic))
        })
    }
}

impl fmt::Debug for MaterialFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MaterialFactory")
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
    /// Worker threads for the element work. 0 means all cores of the machine. The solver
    /// uses at most [`MAX_THREADS`].
    pub threads: usize,
    /// Write a restart file after every N increments. The end of a step always writes one.
    pub restart_every: usize,
    /// Also write a restart file when this many minutes passed since the last one.
    pub restart_minutes: Option<f64>,
    /// Keep the factor of the first tangent of an increment for all its iterations.
    pub modified_newton: bool,
    /// Solves that one increment may use before the solver cuts it back.
    pub newton_max_iterations: usize,
    /// The residual must fall below this part of the force scale.
    pub newton_tolerance: f64,
    /// A cutback multiplies the increment by this.
    pub cutback_factor: f64,
    /// A quick increment (4 solves or fewer) multiplies the next one by this.
    pub growth_factor: f64,
    /// Builds the materials. `None` is [`MaterialFactory::elastic`].
    pub materials: Option<MaterialFactory>,
    /// Builds the contributions that are not elements (contact in M6). `None` adds none.
    pub contributions: Option<ContributionFactory>,
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
            restart_every: 1,
            restart_minutes: None,
            modified_newton: false,
            newton_max_iterations: 25,
            newton_tolerance: 1e-9,
            cutback_factor: 0.25,
            growth_factor: 1.5,
            materials: None,
            contributions: None,
        }
    }
}

/// Switch rule of `Method::Auto`. See DESIGN.md for the measurements.
/// Decision D-06, see DESIGN.md: a direct factor above this size does not fit the target machine.
pub const DIRECT_MAX_DOFS: usize = 60_000;

/// A solve fails if the relative residual of the reduced system is above this.
pub const RESIDUAL_LIMIT: f64 = 1e-6;

/// What the solver did in one step. During a step the numbers are the totals so far.
#[derive(Debug, Clone, PartialEq)]
pub struct StepInfo {
    pub free_dofs: usize,
    /// The method that ran (never `Auto`).
    pub method: Method,
    /// Conjugate gradient iterations. 0 for Cholesky.
    pub iterations: usize,
    /// Relative residual after the last solve (linear) or of the last increment (Newton).
    pub residual: f64,
    /// Increments that converged.
    pub increments: usize,
    /// Increments that were cut back.
    pub cutbacks: usize,
    /// Solves of the Newton loop. 0 for a linear step.
    pub newton_iterations: usize,
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
    /// Tangent matrices that the run built.
    pub assemblies: usize,
    /// Factorizations that the run made.
    pub factorizations: usize,
    /// Seconds in the element sweeps (the element work and the addition into the matrix).
    pub assemble_seconds: f64,
    /// Seconds in the factorizations.
    pub factor_seconds: f64,
    /// Seconds in the solves with a factor, and in the iterative solves.
    pub solve_seconds: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub steps: Vec<StepResult>,
    pub summary: Summary,
}

/// The end of an increment. The committed state is final.
pub struct Increment<'a> {
    /// Step, from 0.
    pub step: usize,
    /// Increment of the step, from 1.
    pub number: usize,
    /// True for the increment that ends the step.
    pub last: bool,
    /// Step time and load factor (0 to 1) at the end of the increment.
    pub time: f64,
    pub lambda: f64,
    /// Size that the next increment will try, as a part of the step.
    pub next_size: f64,
    /// Totals of the step so far.
    pub info: StepInfo,
    /// Committed displacement of all dofs.
    pub displacement: &'a [f64],
    /// Displacement at the start of the step, if the run needs it to continue inside the step.
    /// Empty when the step is done.
    pub base: &'a [f64],
    /// Material state of the integration points and internal dofs of the elements.
    pub states: &'a StateStore,
    /// State of the contribution groups (contact in M6).
    pub contact: &'a StateStore,
    /// Linear solves of this increment: 1 for the linear fallback, the Newton iterations otherwise.
    pub solves: usize,
}

/// A point to continue a run from. A restart file holds it.
#[derive(Debug, Clone)]
pub struct Resume {
    /// Step of the last committed increment, from 0.
    pub step: usize,
    /// Increments of that step that are done.
    pub number: usize,
    /// True when that increment ended the step.
    pub complete: bool,
    pub time: f64,
    pub lambda: f64,
    pub next_size: f64,
    pub displacement: Vec<f64>,
    pub base: Vec<f64>,
    pub image: StateImage,
    /// State of the contribution groups.
    pub contact: Vec<f64>,
    /// Totals of the step so far, when it is not complete.
    pub info: Option<StepInfo>,
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
    /// The increment `number` of step `step` (from 0) was cut back after `solves` solves.
    fn cutback(&mut self, _step: usize, _number: usize, _solves: usize) {}
    /// An increment is done. The observer owns the result.
    fn increment_done(
        &mut self,
        _increment: &Increment,
        _result: StepResult,
    ) -> Result<(), SolverError> {
        Ok(())
    }
}

/// Keeps the result at the end of each step.
struct Collect(Vec<StepResult>);

impl Observer for Collect {
    fn increment_done(&mut self, inc: &Increment, r: StepResult) -> Result<(), SolverError> {
        if inc.last {
            self.0.push(r);
        }
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

/// Solve every step of the model and keep the result at the end of each step.
pub fn solve_options(model: &Model, options: &Options) -> Result<Solution, SolverError> {
    let mut collect = Collect(Vec::new());
    let summary = solve_with(model, options, &mut collect)?;
    Ok(Solution {
        steps: collect.0,
        summary,
    })
}

/// Solve every step. Each increment goes to `observer` as soon as it is done.
pub fn solve_with(
    model: &Model,
    options: &Options,
    observer: &mut dyn Observer,
) -> Result<Summary, SolverError> {
    solve_from(model, options, observer, None)
}

/// Like [`solve_with`], but continue from a restart point. The steps before it only add their
/// data. The loads and constraints of those steps still carry over.
pub fn solve_from(
    model: &Model,
    options: &Options,
    observer: &mut dyn Observer,
    resume: Option<&Resume>,
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
    }
    .min(MAX_THREADS);
    let factory = options
        .materials
        .clone()
        .unwrap_or_else(MaterialFactory::elastic);
    let contribs = match &options.contributions {
        Some(f) => f.build(model)?,
        None => Vec::new(),
    };
    let mesh = Mesh::new(model, &factory, contribs)?;
    let graph = NodeGraph::new(model.nodes.len(), &mesh.conn, &mesh.conn_start);
    // The matrix has about 9 entries for each pair of neighbour nodes, half of them in the lower triangle.
    let nnz_guess = (9 * graph.entries() + 3 * model.nodes.len()) / 2;
    let method_guess = options.method != Method::Cholesky && mesh.n_dofs > options.direct_max_dofs;
    let lens = mesh.state_lens();
    let slot_bytes = mesh
        .conn_start
        .windows(2)
        .map(|w| {
            let n = 3 * (w[1] - w[0]) as usize;
            4 * n * (n + 1) / 2 + 8
        })
        .sum();
    let estimate = estimate_bytes(&Sizes {
        nodes: model.nodes.len(),
        elements: model.elements.len(),
        dofs: mesh.n_dofs,
        nnz: nnz_guess,
        graph_bytes: graph.bytes(),
        threads,
        iterative: method_guess || options.method == Method::Pcg,
        ip_stress: options.formats.text,
        state_bytes: 8 * lens.iter().sum::<usize>()
            + 8 * mesh.group_state_lens().iter().sum::<usize>()
            + if mesh.linear() {
                0
            } else {
                72 * model.elements.len()
            },
        slot_bytes,
    });
    observer.estimate(estimate);
    crate::step::run(&mesh, &graph, options, threads, estimate, observer, resume)
}

/// The largest thread count. The result does not depend on the thread count, and more
/// threads give no gain on the target machine. The bound keeps the chunk sizes of the
/// element work from an overflow.
pub const MAX_THREADS: usize = 1024;

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
    /// Bytes of one copy of the material and element state.
    pub state_bytes: usize,
    /// Bytes of the slot map: 4 for each entry of the lower triangle of each element matrix
    /// (300 for a hexahedron, 465 for a 10-node tetrahedron), and 8 for each element.
    pub slot_bytes: usize,
}

/// Estimate of the peak memory of a run in bytes.
/// Decision D-06, see DESIGN.md: the runner compares it with the memory limit.
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
    // The slot map: one 32-bit slot for each entry of the lower triangle of an element
    // matrix, and the offsets.
    let slots = s.slot_bytes as u64;
    // The state: the committed and the trial copy, a copy for the restart file, and slack.
    let state = 4 * s.state_bytes as u64;
    // The transform, and the full and reduced vectors of the step. The conjugate gradient
    // solver adds a row copy of the strict lower triangle and its vectors.
    let vectors = 24 * n
        + 48 * n
        + if s.iterative {
            72 * n + PcgSolver::bytes(s.dofs, s.nnz) as u64
        } else {
            24 * n
        };
    // The Cholesky factor of a 3-D mesh: fill grows as n^1.35 (measured, DESIGN.md section 11).
    let factor = if s.iterative {
        0
    } else {
        (400.0 * (n as f64).powf(1.35)) as u64
    };
    // The results of one step and the output grid.
    let results =
        72 * nodes + 56 * els + 120 * nodes + 70 * els + if s.ip_stress { 8 * 48 * els } else { 0 };
    // The chunk buffers of the threads: at most one evaluation for each element.
    let chunks = (s.threads as u64).saturating_mul(256).min(els.max(1))
        * std::mem::size_of::<Evaluation>() as u64;
    // Allocator slack, the input text, and parts the terms miss. The measured peak is 1.21 to
    // 1.26 times the sum (iterative path, 1M elements: 1.26). The factor 1.35 keeps the
    // estimate above the peak (DESIGN.md section 11).
    let sum = model + mesh + matrix + slots + state + vectors + factor + results + chunks;
    sum + sum * 7 / 20
}
