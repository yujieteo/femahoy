//! The step driver: one increment loop for the linear fallback and the Newton path.
//!
//! A step is a list of increments. The load factor `lambda` runs from 0 to 1 in the step.
//! Concentrated loads, pressures and prescribed displacements ramp with it. Each increment
//! builds its constraint transform again from the prescribed values at its load factor,
//! so the transform can change from one increment to the next (Decision D-10, see DESIGN.md).
//! A transform with the same map keeps the matrix and the factor. A transform with the same
//! structure keeps the pattern and the solver.
//!
//! * Linear fallback (every material linear and no contribution such as contact): a step is
//!   one increment through the same loop. `K` is built once for a constraint map and factored
//!   once, and every later increment with the same map reuses the factor. No Newton code runs.
//! * Newton path: each increment runs a Newton loop. The loop builds the residual and the
//!   tangent in one sweep over the elements and the contribution groups, solves, and checks
//!   convergence. A converged increment commits the trial state. A rejected increment drops
//!   it and the solver tries a smaller increment (a cutback). With `modified_newton` the
//!   tangent of the first iteration stays for the increment.
//!
//! A sweep is one call of a pure kernel for each element and each contribution group.
//! Threads compute the results of a chunk. One thread then adds them to the arrays in entry
//! order. Thus the result does not depend on the number of threads. Decision D-10, see DESIGN.md.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use fh_inp::Step;

use crate::assembly::{Assemble, CpuAssembler, SlotMap};
use crate::constraint::Transform;
use crate::element::{self, Evaluation, Kind, Placement, MAX_DOFS};
use crate::mesh::Mesh;
use crate::sparse::{CholeskySolver, LinearSolver, Matrix, NodeGraph, PcgSolver};
use crate::state::{StateImage, StateStore};
use crate::system::{
    step_times, Increment, Method, Observer, Options, Resume, StepInfo, StepResult, Summary,
    CANCELLED, RESIDUAL_LIMIT,
};
use crate::SolverError;

/// Why an attempt stopped. A rejection leads to a cutback on the Newton path. A fatal error
/// ends the run.
enum Fail {
    Reject(SolverError),
    Fatal(SolverError),
}

impl Fail {
    fn into_error(self) -> SolverError {
        match self {
            Fail::Reject(e) | Fail::Fatal(e) => e,
        }
    }
}

/// The matrix of the run with everything that depends on its pattern.
struct Cache {
    t: Transform,
    matrix: Matrix,
    map: SlotMap,
    method: Method,
    solver: Box<dyn LinearSolver>,
    /// The matrix holds the tangent for the map of `t`.
    built: bool,
    /// The solver holds the factor of the matrix.
    factored: bool,
}

#[derive(Default)]
struct Stats {
    assemblies: usize,
    factorizations: usize,
    assemble: f64,
    factor: f64,
    solve: f64,
}

/// A pressure on a face. It ramps from `from` to `to` inside the step.
struct Face {
    element: usize,
    face: u8,
    from: f64,
    to: f64,
}

/// The external loads of a step.
struct Loads {
    prev: Vec<f64>,
    cur: Vec<f64>,
    faces: Vec<Face>,
}

impl Loads {
    /// External nodal forces at the load factor `lambda`. Pressures act on the faces at the
    /// positions start + `u`. Without `u` the faces are at the start positions.
    fn external(
        &self,
        mesh: &Mesh,
        lambda: f64,
        u: Option<&[f64]>,
    ) -> Result<Vec<f64>, SolverError> {
        let mut f = if lambda == 1.0 {
            self.cur.clone()
        } else {
            self.prev
                .iter()
                .zip(&self.cur)
                .map(|(a, b)| a + lambda * (b - a))
                .collect()
        };
        for face in &self.faces {
            let p = if lambda == 1.0 {
                face.to
            } else {
                face.from + lambda * (face.to - face.from)
            };
            let e = face.element;
            let mut coords = [[0.0; 3]; 10];
            let n = mesh.element_coords(e, &mut coords);
            let mut ue = [0.0; MAX_DOFS];
            if let Some(u) = u {
                let mut dofs = [0; MAX_DOFS];
                let nd = mesh.element_dofs(e, &mut dofs);
                for (v, &d) in ue.iter_mut().zip(&dofs[..nd]) {
                    *v = u[d];
                }
            }
            let place = Placement {
                start: &coords[..n],
                disp: &ue[..3 * n],
            };
            for (local, force) in element::pressure_load(mesh.kinds[e], &place, face.face, p)
                .map_err(|err| mesh.element_error(e, err))?
            {
                let node = mesh.conn[mesh.conn_start[e] as usize + local] as usize;
                for (k, v) in force.iter().enumerate() {
                    f[mesh.dof(node, k as u8 + 1)] += v;
                }
            }
        }
        Ok(f)
    }
}

/// What an element sweep collected.
struct Sweep {
    fint: Vec<f64>,
    fabs: Vec<f64>,
    stress: Vec<[f64; 6]>,
    ip_stress: Vec<[f64; 6]>,
}

/// What a sweep adds up. `consume` owns it on the calling thread.
struct Acc<'a> {
    states: &'a mut StateStore,
    contact: &'a mut StateStore,
    values: Option<&'a mut [f64]>,
    store: bool,
    keep_ip: bool,
    out: Sweep,
}

/// Entries (elements or groups) per thread in one chunk.
const CHUNK: usize = 256;

/// Run `work` for the entries `0..n`. Threads compute chunks in parallel,
/// and `consume` sees the results in entry order on this thread.
fn for_elements<S: Sync>(
    threads: usize,
    n: usize,
    s: &mut S,
    work: &(dyn Fn(&S, usize, &mut Evaluation) -> Result<(), SolverError> + Sync),
    consume: &mut dyn FnMut(&mut S, usize, &[Evaluation]),
    tick: &mut dyn FnMut(f64) -> Result<(), SolverError>,
) -> Result<(), Fail> {
    let threads = threads.max(1);
    let chunk = threads * CHUNK;
    let mut buf = vec![Evaluation::default(); chunk.min(n.max(1))];
    let mut start = 0;
    while start < n {
        let len = chunk.min(n - start);
        let shared: &S = s;
        if threads == 1 || len < 2 * threads {
            for (i, ev) in buf[..len].iter_mut().enumerate() {
                work(shared, start + i, ev).map_err(Fail::Reject)?;
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
                                work(shared, start + t * per + i, ev)?;
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
            // The first error in entry order wins.
            for r in results {
                r.map_err(Fail::Reject)?;
            }
        }
        consume(s, start, &buf[..len]);
        start += len;
        tick(start as f64 / n as f64).map_err(Fail::Fatal)?;
    }
    Ok(())
}

fn norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// A result with a value that is not finite is not a solution. The residual test does not
/// see it when no dof is free, or when the value comes from the recovery.
fn check_finite(u: &[f64], f_ext: &[f64], sweep: &Sweep) -> Result<(), SolverError> {
    let mut all = u
        .iter()
        .chain(f_ext)
        .chain(&sweep.fint)
        .chain(sweep.stress.iter().flatten())
        .chain(sweep.ip_stress.iter().flatten());
    if all.all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(SolverError::new(
            "the result has a value that is not finite: check the size of the loads, the prescribed values and the coordinates",
        ))
    }
}

/// The result of one increment that the loop accepts.
struct Converged {
    u: Vec<f64>,
    f_ext: Vec<f64>,
    sweep: Sweep,
    /// Linear solves: the Newton iterations. Zero for the linear fallback.
    solves: usize,
    /// Iterations of the iterative linear solver.
    iterations: usize,
    residual: f64,
}

struct Run<'a> {
    mesh: &'a Mesh<'a>,
    graph: &'a NodeGraph,
    options: &'a Options,
    threads: usize,
    n_steps: usize,
    /// State of the elements.
    states: StateStore,
    /// State of the contribution groups.
    contact: StateStore,
    cache: Option<Cache>,
    stats: Stats,
    pattern_builds: usize,
    nnz: usize,
    /// Structure of the last step that was skipped by a restart. Its pattern is counted but not built.
    counted: Option<Transform>,
}

/// Solve all steps. See [`crate::solve_from`].
pub(crate) fn run(
    mesh: &Mesh,
    graph: &NodeGraph,
    options: &Options,
    threads: usize,
    estimate: u64,
    observer: &mut dyn Observer,
    resume: Option<&Resume>,
) -> Result<Summary, SolverError> {
    let model = mesh.model;
    let n_steps = model.steps.len();
    let internal = !mesh.all_linear() && mesh.kinds.contains(&Kind::C3D8I);
    let mut states = StateStore::new(&mesh.state_lens(), internal);
    let mut contact = StateStore::new(&mesh.group_state_lens(), false);
    let mut u_c = vec![0.0; mesh.n_dofs];
    if let Some(r) = resume {
        if r.step >= n_steps || r.displacement.len() != mesh.n_dofs {
            return Err(SolverError::new(
                "the restart file does not fit the model: the sizes differ",
            ));
        }
        states.load_image(&r.image)?;
        contact.load_image(&StateImage {
            ip: r.contact.clone(),
            internal: Vec::new(),
        })?;
        u_c = r.displacement.clone();
    }
    let mut run = Run {
        mesh,
        graph,
        options,
        threads,
        n_steps,
        states,
        contact,
        cache: None,
        stats: Stats::default(),
        pattern_builds: 0,
        nnz: 0,
        counted: None,
    };
    let mut summary = Summary {
        total_dofs: mesh.n_dofs,
        steps: Vec::new(),
        pattern_builds: 0,
        nnz: 0,
        orphans: Vec::new(),
        estimate_bytes: estimate,
        assemblies: 0,
        factorizations: 0,
        assemble_seconds: 0.0,
        factor_seconds: 0.0,
        solve_seconds: 0.0,
    };
    let mut bc: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut loads: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut pressures: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    mesh.add_bcs(&model.boundaries, &mut bc)?;
    let mut orphans: BTreeSet<(u32, u8)> = BTreeSet::new();
    let times = step_times(model);
    let mut prev = vec![0.0; mesh.n_dofs];
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
        let pressures_before = pressures.clone();
        for dl in &step.dloads {
            for e in mesh.elements_of(&dl.target)? {
                pressures.insert((e, dl.face), dl.pressure);
            }
        }
        let mut cur = vec![0.0; mesh.n_dofs];
        for (&(n, d), &v) in &loads {
            cur[mesh.dof(n, d)] += v;
        }
        let keys: BTreeSet<(usize, u8)> = pressures
            .keys()
            .chain(pressures_before.keys())
            .copied()
            .collect();
        let faces = keys
            .into_iter()
            .map(|k| Face {
                element: k.0,
                face: k.1,
                from: pressures_before.get(&k).copied().unwrap_or(0.0),
                to: pressures.get(&k).copied().unwrap_or(0.0),
            })
            .collect();
        let set = Loads {
            prev: std::mem::replace(&mut prev, cur.clone()),
            cur,
            faces,
        };
        let done = resume.is_some_and(|r| index < r.step || (index == r.step && r.complete));
        if done {
            // The step ran before the restart. Count its pattern and its orphan dofs.
            let t = run.constrain(&bc, &set.cur, &mut orphans)?;
            run.count_structure(&t);
            continue;
        }
        if observer.cancelled() {
            return Err(SolverError::new(CANCELLED));
        }
        let start = resume.filter(|r| r.step == index && !r.complete);
        let t0 = if index == 0 { 0.0 } else { times[index - 1] };
        let info = run.step(
            observer,
            index,
            step,
            &bc,
            &set,
            (t0, times[index]),
            &mut u_c,
            start,
            &mut orphans,
        )?;
        summary.steps.push(info);
    }
    if run.nnz == 0 {
        if let Some(t) = run.counted.take() {
            let base = &mesh.base;
            run.nnz = Matrix::pattern(graph, &|n, c| base[n] + c, &t)?.nnz();
        }
    }
    summary.orphans = orphans.into_iter().collect();
    summary.pattern_builds = run.pattern_builds;
    summary.nnz = run.nnz;
    summary.assemblies = run.stats.assemblies;
    summary.factorizations = run.stats.factorizations;
    summary.assemble_seconds = run.stats.assemble;
    summary.factor_seconds = run.stats.factor;
    summary.solve_seconds = run.stats.solve;
    Ok(summary)
}

/// The prescribed values at the load factor `lambda`. A value ramps from the displacement
/// at the start of the step (`base`) to its value in the step.
fn ramp(
    mesh: &Mesh,
    bc: &BTreeMap<(usize, u8), f64>,
    base: &[f64],
    lambda: f64,
) -> BTreeMap<(usize, u8), f64> {
    if lambda == 1.0 {
        return bc.clone();
    }
    bc.iter()
        .map(|(&(n, d), &v)| {
            let b = base[mesh.dof(n, d)];
            ((n, d), b + lambda * (v - b))
        })
        .collect()
}

/// The incrementation rule of a step as parts of the step: first, smallest and largest size.
fn increment_rule(step: &Step, period: f64) -> (f64, f64, f64) {
    let sp = step.static_procedure.clone().unwrap_or_default();
    let positive = |v: Option<f64>| v.filter(|x| *x > 0.0 && x.is_finite());
    let max = positive(sp.max_increment).map_or(1.0, |v| (v / period).min(1.0));
    let first = positive(sp.initial_increment)
        .map_or(1.0, |v| (v / period).min(1.0))
        .min(max);
    let min = positive(sp.min_increment)
        .map_or(1e-5, |v| v / period)
        .min(first);
    (first, min, max)
}

impl Run<'_> {
    /// The constraint transform for the prescribed values `bc`. A dof that no element
    /// reaches has no stiffness. It is fixed at zero.
    fn constrain(
        &self,
        bc: &BTreeMap<(usize, u8), f64>,
        f: &[f64],
        orphans: &mut BTreeSet<(u32, u8)>,
    ) -> Result<Transform, SolverError> {
        let mesh = self.mesh;
        let mut el = mesh.elimination(bc)?;
        let mut t = el.finish();
        let fixed = self.orphan_dofs(&t);
        if !fixed.is_empty() {
            // A constraint can move a load to a free dof, so check the reduced load T^T f.
            // The load at the full dof alone can be zero.
            let reduced = t.reduce(f);
            for &r in &fixed {
                let d = t.full_dof(r);
                let node = mesh.base.partition_point(|&b| b <= d) - 1;
                let (id, comp) = (mesh.model.nodes[node].id, (d - mesh.base[node]) as u8 + 1);
                if reduced[r] != 0.0 {
                    return Err(SolverError::new(format!(
                        "node {id} has a load on dof {comp}, directly or through a constraint, but no element gives it stiffness"
                    )));
                }
                orphans.insert((id, comp));
                el.add(&[(d, 1.0)], 0.0, Some(d))?;
            }
            t = el.finish();
        }
        Ok(t)
    }

    /// Free dofs (reduced numbers) that no element stiffness reaches.
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
        (0..t.n_free).filter(|&r| !used[r]).collect()
    }

    /// A step that a restart skips still counts its pattern, so the counter equals the one of a full run.
    fn count_structure(&mut self, t: &Transform) {
        if !self.counted.as_ref().is_some_and(|c| c.same_structure(t)) {
            self.pattern_builds += 1;
            self.counted = Some(t.clone());
        }
    }

    fn new_solver(&self, method: Method) -> Box<dyn LinearSolver> {
        match method {
            Method::Pcg => Box::new(PcgSolver::new(
                self.options.tolerance,
                self.options.max_iterations,
                self.threads,
            )),
            _ => Box::new(CholeskySolver::default()),
        }
    }

    /// The matrix, the slot map and the solver for the transform `t`. The old ones stay when
    /// they fit: with the same map the matrix and the factor stay, with the same structure
    /// the pattern and the solver (and its symbolic analysis) stay.
    fn prepare(&mut self, t: Transform) -> Result<Cache, SolverError> {
        let mesh = self.mesh;
        let method = match self.options.method {
            Method::Auto if t.n_free <= self.options.direct_max_dofs => Method::Cholesky,
            Method::Auto => Method::Pcg,
            m => m,
        };
        let dofs_of = |e: usize, buf: &mut [usize; MAX_DOFS]| mesh.element_dofs(e, buf);
        let n_entries = mesh.n_entries();
        let counted = self.counted.take();
        match self.cache.take() {
            Some(mut c) if c.method == method && c.t.same_map(&t) => {
                c.t = t;
                Ok(c)
            }
            Some(mut c) if c.method == method && c.t.same_structure(&t) => {
                c.t = t;
                c.matrix.clear();
                c.map = SlotMap::build(&c.matrix, &c.t, n_entries, &dofs_of);
                c.built = false;
                c.factored = false;
                Ok(c)
            }
            old => {
                drop(old);
                let base = &mesh.base;
                let matrix = Matrix::pattern(self.graph, &|n, c| base[n] + c, &t)?;
                if !counted.is_some_and(|c| c.same_structure(&t)) {
                    self.pattern_builds += 1;
                }
                self.nnz = matrix.nnz();
                let map = SlotMap::build(&matrix, &t, n_entries, &dofs_of);
                Ok(Cache {
                    t,
                    matrix,
                    map,
                    method,
                    solver: self.new_solver(method),
                    built: false,
                    factored: false,
                })
            }
        }
    }

    /// Run the increments of one step. `bc` holds the prescribed values at the end of the step.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        observer: &mut dyn Observer,
        index: usize,
        step: &Step,
        bc: &BTreeMap<(usize, u8), f64>,
        loads: &Loads,
        span: (f64, f64),
        u_c: &mut Vec<f64>,
        start: Option<&Resume>,
        orphans: &mut BTreeSet<(u32, u8)>,
    ) -> Result<StepInfo, SolverError> {
        let mesh = self.mesh;
        let opts = self.options;
        let (idx, n_steps) = (index + 1, self.n_steps);
        let (t0, t1) = span;
        let period = (t1 - t0).max(f64::MIN_POSITIVE);
        let linear = mesh.linear();
        // A linear response is the same for any increment size: the step is one increment.
        let (first, min, max) = if linear {
            (1.0, 1.0, 1.0)
        } else {
            increment_rule(step, period)
        };
        // The displacement at the start of the step. Prescribed values ramp from it.
        let base: Vec<f64> = match start {
            Some(r) if r.base.len() == mesh.n_dofs => r.base.clone(),
            Some(_) => {
                return Err(SolverError::new(
                    "the restart file does not fit the model: the sizes differ",
                ))
            }
            None => u_c.clone(),
        };
        let mut lambda = start.map_or(0.0, |r| r.lambda);
        let mut number = start.map_or(0, |r| r.number);
        let mut size = start.map_or(first, |r| r.next_size.clamp(min, max));
        let mut info = StepInfo {
            free_dofs: 0,
            method: Method::Auto,
            iterations: 0,
            residual: 0.0,
            increments: 0,
            cutbacks: 0,
            newton_iterations: 0,
        };
        if let Some(i) = start.and_then(|r| r.info.as_ref()) {
            info = i.clone();
        }
        while lambda < 1.0 {
            if observer.cancelled() {
                return Err(SolverError::new(CANCELLED));
            }
            let attempt = size.min(max);
            let remaining = 1.0 - lambda;
            // Land on 1 when the rest is not larger than the attempt, or would be below the minimum.
            let (target, actual) =
                if remaining <= attempt * (1.0 + 1e-9) || remaining - attempt < min {
                    (1.0, remaining)
                } else {
                    (lambda + attempt, attempt)
                };
            // The constraint transform of this increment.
            let t = self.constrain(&ramp(mesh, bc, &base, target), &loads.cur, orphans)?;
            let mut c = self.prepare(t)?;
            info.free_dofs = c.t.n_free;
            info.method = c.method;
            let mut solves = 0;
            let out = if linear {
                self.linear_increment(observer, idx, &mut c, loads)
            } else {
                let ur: Vec<f64> = (0..c.t.n_free).map(|r| u_c[c.t.full_dof(r)]).collect();
                self.newton(observer, idx, &mut c, loads, ur, target, &mut solves)
            };
            // The cache stays for the next increment, also after an error.
            self.cache = Some(c);
            match out {
                Ok(done) => {
                    self.states.commit();
                    self.contact.commit();
                    lambda = target;
                    number += 1;
                    info.increments += 1;
                    info.newton_iterations += done.solves;
                    info.iterations += done.iterations;
                    info.residual = done.residual;
                    size = if done.solves <= 4 {
                        actual * opts.growth_factor
                    } else {
                        actual
                    }
                    .min(max);
                    let last = lambda >= 1.0;
                    let time = if last { t1 } else { t0 + lambda * period };
                    let solves = if linear {
                        usize::from(info.free_dofs > 0)
                    } else {
                        done.solves
                    };
                    let Converged {
                        u,
                        f_ext,
                        mut sweep,
                        ..
                    } = done;
                    for (r, fx) in sweep.fint.iter_mut().zip(&f_ext) {
                        *r -= fx;
                    }
                    let reaction = std::mem::take(&mut sweep.fint);
                    let result = self.result(step, time, &u, &reaction, sweep);
                    *u_c = u;
                    if linear {
                        observer.progress(idx, n_steps, "recover", 1.0);
                    } else {
                        observer.progress(idx, n_steps, "increment", lambda);
                    }
                    observer.increment_done(
                        &Increment {
                            step: index,
                            number,
                            last,
                            time,
                            lambda,
                            next_size: size,
                            info: info.clone(),
                            displacement: u_c,
                            base: if last { &[] } else { &base },
                            states: &self.states,
                            contact: &self.contact,
                            solves,
                        },
                        result,
                    )?;
                }
                Err(Fail::Reject(reason)) if !linear => {
                    info.cutbacks += 1;
                    self.states.cutback();
                    self.contact.cutback();
                    observer.cutback(index, number + 1, solves);
                    size = attempt * opts.cutback_factor;
                    if size < min {
                        return Err(SolverError::new(format!(
                            "step {idx}: the increment fell below the minimum increment of {:e} at load factor {lambda:.6}: {reason}",
                            min * period
                        )));
                    }
                }
                Err(e) => return Err(e.into_error()),
            }
        }
        Ok(info)
    }

    /// One sweep at the displacements `u` over the elements and the contribution groups.
    /// With `tangent`, the matrices go into the values of the matrix through the slot map.
    /// With `store`, the trial state goes into the state stores.
    #[allow(clippy::too_many_arguments)]
    fn sweep(
        &mut self,
        observer: &mut dyn Observer,
        index: usize,
        phase: &str,
        report: bool,
        u: &[f64],
        tangent: Option<(&mut [f64], &SlotMap)>,
        store: bool,
    ) -> Result<Sweep, Fail> {
        let clock = Instant::now();
        let mesh = self.mesh;
        let n_steps = self.n_steps;
        let n_el = mesh.kinds.len();
        let keep_ip = self.options.formats.text;
        let (values, map) = match tangent {
            Some((v, m)) => (Some(v), Some(m)),
            None => (None, None),
        };
        let with_k = values.is_some();
        let mut acc = Acc {
            states: &mut self.states,
            contact: &mut self.contact,
            values,
            store,
            keep_ip,
            out: Sweep {
                fint: vec![0.0; mesh.n_dofs],
                fabs: vec![0.0; mesh.n_dofs],
                stress: vec![[0.0; 6]; n_el],
                ip_stress: Vec::new(),
            },
        };
        let work = |acc: &Acc<'_>, e: usize, ev: &mut Evaluation| {
            let mut dofs = [0; MAX_DOFS];
            let n = mesh.element_dofs(e, &mut dofs);
            let mut ue = [0.0; MAX_DOFS];
            for (v, &d) in ue.iter_mut().zip(&dofs[..n]) {
                *v = u[d];
            }
            if e < n_el {
                let state = mesh.element_state(&*acc.states, e);
                mesh.evaluate_element(e, &ue, &state, ev, with_k)
            } else {
                let g = e - n_el;
                mesh.evaluate_group(g, &ue, acc.contact.committed(g), ev, with_k)
            }
        };
        let mut consume = |acc: &mut Acc<'_>, first: usize, evs: &[Evaluation]| {
            if let (Some(values), Some(map)) = (acc.values.as_deref_mut(), map) {
                CpuAssembler.add_chunk(values, map, first, evs);
            }
            for (k, ev) in evs.iter().enumerate() {
                let e = first + k;
                let mut dofs = [0; MAX_DOFS];
                let n = mesh.element_dofs(e, &mut dofs);
                for (i, &d) in dofs[..n].iter().enumerate() {
                    acc.out.fint[d] += ev.fint[i];
                    acc.out.fabs[d] += ev.fabs[i];
                }
                if e >= n_el {
                    if acc.store {
                        let g = e - n_el;
                        let len = acc.contact.committed(g).len();
                        acc.contact.set_trial(g, &ev.state()[..len]);
                    }
                    continue;
                }
                acc.out.stress[e] = ev.mean_stress;
                if acc.keep_ip {
                    acc.out
                        .ip_stress
                        .extend_from_slice(&ev.ip_stress[..mesh.kinds[e].point_count()]);
                }
                if acc.store {
                    mesh.store_trial(acc.states, e, ev);
                }
            }
        };
        let mut tick = |fraction: f64| {
            if report {
                observer.progress(index, n_steps, phase, fraction);
            }
            if observer.cancelled() {
                Err(SolverError::new(CANCELLED))
            } else {
                Ok(())
            }
        };
        let n = mesh.n_entries();
        let done = for_elements(self.threads, n, &mut acc, &work, &mut consume, &mut tick);
        let out = acc.out;
        self.stats.assemble += clock.elapsed().as_secs_f64();
        done.map(|()| out)
    }

    fn result(
        &self,
        step: &Step,
        time: f64,
        u: &[f64],
        reaction: &[f64],
        sweep: Sweep,
    ) -> StepResult {
        let mesh = self.mesh;
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
        StepResult {
            name: step.name.clone(),
            time,
            displacement: grab(u, 1),
            rotation: grab(u, 4),
            reaction: grab(reaction, 1),
            mises: sweep.stress.iter().map(element::von_mises).collect(),
            element_stress: sweep.stress,
            ip_stress: sweep.ip_stress,
        }
    }

    /// Factor the matrix of the cache.
    fn factor(&mut self, c: &mut Cache) -> Result<(), SolverError> {
        let clock = Instant::now();
        let out = c.solver.factor(&c.matrix);
        self.stats.factor += clock.elapsed().as_secs_f64();
        out?;
        self.stats.factorizations += 1;
        c.factored = true;
        Ok(())
    }

    /// Solve `K x = rhs` with the factor of the cache. Returns `x` and the iteration count.
    fn solve(
        &mut self,
        observer: &mut dyn Observer,
        index: usize,
        c: &Cache,
        rhs: &[f64],
    ) -> Result<(Vec<f64>, usize), Fail> {
        let clock = Instant::now();
        let n_steps = self.n_steps;
        let tol = self.options.tolerance;
        let mut stop = false;
        let out = c.solver.solve(&c.matrix, rhs, &mut |_, rel| {
            let done = (1.0 / rel.max(1e-300)).ln() / (1.0 / tol).ln();
            observer.progress(index, n_steps, "solve", done.clamp(0.0, 0.99));
            stop = observer.cancelled();
            !stop
        });
        self.stats.solve += clock.elapsed().as_secs_f64();
        match out {
            Ok(x) => Ok(x),
            Err(_) if stop => Err(Fail::Fatal(SolverError::new(CANCELLED))),
            Err(e) => Err(Fail::Reject(e)),
        }
    }

    /// The linear fallback for one increment at the load factor 1. `K` is built once for a
    /// constraint map and factored once. A later increment with the same map reuses the
    /// factor. The prescribed values enter as `-K u_0` on the right side.
    fn linear_increment(
        &mut self,
        observer: &mut dyn Observer,
        idx: usize,
        c: &mut Cache,
        loads: &Loads,
    ) -> Result<Converged, Fail> {
        let mesh = self.mesh;
        let n_steps = self.n_steps;
        let f = loads.external(mesh, 1.0, None).map_err(Fail::Fatal)?;
        observer.progress(idx, n_steps, "assemble", 0.0);
        let u0 = c.t.offset.clone();
        let mut fint0: Option<Vec<f64>> = None;
        if !c.built {
            c.matrix.clear();
            let tangent = Some((c.matrix.values_mut(), &c.map));
            let sweep = self.sweep(observer, idx, "assemble", true, &u0, tangent, false)?;
            self.stats.assemblies += 1;
            c.built = true;
            c.factored = false;
            fint0 = Some(sweep.fint);
        } else if u0.iter().any(|v| *v != 0.0) {
            let sweep = self.sweep(observer, idx, "assemble", true, &u0, None, false)?;
            fint0 = Some(sweep.fint);
        }
        let mut rhs = f.clone();
        if let Some(f0) = &fint0 {
            for (r, x) in rhs.iter_mut().zip(f0) {
                *r -= x;
            }
        }
        let fr = c.t.reduce(&rhs);
        drop(rhs);
        drop(fint0);
        let mut iterations = 0;
        let mut ur = vec![0.0; c.t.n_free];
        if c.t.n_free > 0 {
            observer.progress(idx, n_steps, "solve", 0.0);
            if !c.factored {
                self.factor(c).map_err(Fail::Fatal)?;
            }
            let (x, its) = self.solve(observer, idx, c, &fr)?;
            ur = x;
            iterations = its;
        }
        // Residual of the reduced system.
        let residual = {
            let mut kx = vec![0.0; c.t.n_free];
            c.matrix.mul(&ur, &mut kx);
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
            return Err(Fail::Fatal(SolverError::new(format!(
                "the solution does not satisfy the equations: relative residual {residual:e}, limit {RESIDUAL_LIMIT:e}. Check the boundary conditions and the constraints"
            ))));
        }
        drop(fr);
        let u = c.t.expand(&ur);
        drop(ur);
        // Reactions and stress from the element forces.
        observer.progress(idx, n_steps, "recover", 0.0);
        let sweep = self.sweep(observer, idx, "recover", true, &u, None, false)?;
        check_finite(&u, &f, &sweep).map_err(Fail::Fatal)?;
        Ok(Converged {
            u,
            f_ext: f,
            sweep,
            solves: 0,
            iterations,
            residual,
        })
    }

    /// The Newton loop of one increment at the load factor `lambda`. `ur` is the first guess
    /// of the reduced unknowns. `solves` counts the linear solves, also of a rejected attempt.
    #[allow(clippy::too_many_arguments)]
    fn newton(
        &mut self,
        observer: &mut dyn Observer,
        idx: usize,
        c: &mut Cache,
        loads: &Loads,
        mut ur: Vec<f64>,
        lambda: f64,
        solves: &mut usize,
    ) -> Result<Converged, Fail> {
        let mesh = self.mesh;
        let opts = self.options;
        let mut iterations = 0;
        loop {
            let u = c.t.expand(&ur);
            let sweep = if !opts.modified_newton || *solves == 0 {
                c.matrix.clear();
                let tangent = Some((c.matrix.values_mut(), &c.map));
                let s = self.sweep(observer, idx, "newton", false, &u, tangent, true)?;
                self.stats.assemblies += 1;
                c.built = true;
                c.factored = false;
                s
            } else {
                self.sweep(observer, idx, "newton", false, &u, None, true)?
            };
            let f_ext = loads
                .external(mesh, lambda, Some(&u))
                .map_err(Fail::Reject)?;
            let full: Vec<f64> = f_ext.iter().zip(&sweep.fint).map(|(a, b)| a - b).collect();
            let r = c.t.reduce(&full);
            drop(full);
            let size = norm(&r);
            let scale = norm(&c.t.reduce(&sweep.fabs)).max(norm(&c.t.reduce(&f_ext)));
            let residual = if scale > 0.0 { size / scale } else { size };
            if size.is_nan() {
                return Err(Fail::Reject(SolverError::new(
                    "the residual is not a number",
                )));
            }
            if size <= opts.newton_tolerance * scale + 1e-30 {
                check_finite(&u, &f_ext, &sweep).map_err(Fail::Reject)?;
                return Ok(Converged {
                    u,
                    f_ext,
                    sweep,
                    solves: *solves,
                    iterations,
                    residual,
                });
            }
            if residual > 1e6 {
                return Err(Fail::Reject(SolverError::new(format!(
                    "the iteration diverges: relative residual {residual:e}"
                ))));
            }
            if *solves >= opts.newton_max_iterations {
                return Err(Fail::Reject(SolverError::new(format!(
                    "no convergence in {solves} iterations: relative residual {residual:e}"
                ))));
            }
            if !c.factored {
                self.factor(c).map_err(Fail::Reject)?;
            }
            let (du, its) = self.solve(observer, idx, c, &r)?;
            for (x, d) in ur.iter_mut().zip(&du) {
                *x += d;
            }
            *solves += 1;
            iterations += its;
        }
    }
}
