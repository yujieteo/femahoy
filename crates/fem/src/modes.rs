//! The natural frequencies and the mode shapes of a linear elastic structure: K x = omega^2 M x,
//! on the box grid with the 8-node hex of incompatible modes (`hex`), or on the extruded mesh
//! with 10-node tetrahedra (`tet10`) for a model with a prism part.
//!
//! The model is a static model (`elastic`) whose loads are only the supports: displacement
//! components held at 0 on face sets or at points. With no support, the body is free and its 6
//! rigid motions are modes with a frequency of 0. The mass is the consistent mass of each cell
//! from the density of its material.
//!
//! The eigensolver is shift-invert subspace iteration (`eigen`). The shift is below 0, a small
//! fraction of the mean of K_ii / M_ii, so K - sigma M is positive definite also for a free
//! body, and the direct factor or PCG with multigrid solves it.
//!
//! Each mode shape has phi^T M phi = 1, and its largest component is positive. The participation
//! factor of a direction d is Gamma_d = phi^T M r_d, r_d the unit translation along d at each
//! free unknown, and the effective mass is Gamma_d^2. The sum of the effective masses of all
//! modes is the mass that moves with the free unknowns.
//!
//! Sources: T. J. R. Hughes, The Finite Element Method, Prentice-Hall 1987, sections 7.1 to 7.3
//! (the consistent mass, the modal equations and the rigid body modes); A. K. Chopra, Dynamics
//! of Structures, 4th ed., Prentice-Hall 2012, section 13.2 (the participation factor and the
//! effective modal mass); K.-J. Bathe, The subspace iteration method - revisited, Computers &
//! Structures 126 (2013) 177-183 (the shift of a free body).

use crate::eigen;
use crate::elastic::{self, check, fix_node, grid_point, Load, Thermal};
use crate::elastic_tet::{corner_at, MAX_POINTS as TET_MAX_POINTS};
use crate::heat::{self, build_grid, number, select_tet_faces, Cell, Mesh};
use crate::hex::{self, Hex};
use crate::multigrid::{Cells, Level};
use crate::pmultigrid::prolongation;
use crate::prism;
use crate::solve::{Crossover, Method, Solver, Unknowns};
use crate::sparse::Csr;
use crate::tet10::{Quadratic, Tet10};

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The parts, the materials, the grid, the face sets and the supports. The loads are
    /// displacements of 0 on face sets or at points; there is no thermal load, no symmetry, no
    /// probe and no surface.
    pub structure: elastic::Model,
    /// The number of modes, the lowest first.
    pub count: usize,
}

/// A mode of the structure.
#[derive(Debug, Clone, PartialEq)]
pub struct Mode {
    /// The frequency in cycles per time unit. A rigid motion has 0.
    pub frequency: f64,
    /// omega^2, the eigenvalue. A rigid motion has a value near 0, which can be below 0.
    pub eigenvalue: f64,
    /// Gamma along x, y and z.
    pub participation: [f64; 3],
    /// The effective mass along x, y and z over the mass of the model.
    pub mass_fraction: [f64; 3],
    /// The displacement at each point of the mesh.
    pub shape: Vec<[f64; 3]>,
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
    fn mode(&mut self, mode: &Mode) -> Result<(), String>;
}

/// The end of a solve.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub cells: usize,
    pub points: usize,
    pub unknowns: usize,
    pub method: Method,
    /// The iterations of the subspace iteration.
    pub iterations: usize,
    /// The mass of the model.
    pub mass: f64,
    pub warnings: Vec<String>,
}

/// The shift is -SHIFT times the mean of K_ii / M_ii of the free unknowns. Reason: the mean is
/// near the largest eigenvalue, and the lowest elastic eigenvalue of a thin plate on a fine grid
/// is about 1e-8 of it, so the shift stays below the lowest eigenvalues, and the convergence
/// rate does not suffer. The smallest pivot of the factor of a free body is then about 1e-8 of
/// its diagonal, 100 times above the pivot limit of `cholesky`.
const SHIFT: f64 = 1e-8;

/// The solver is direct when its factor fits in the memory of a laptop. Reason: the subspace
/// iteration solves 2 q vectors in each iteration, so the factor pays for itself. A cube on
/// the grid (86,490 unknowns, 6 modes, 196 solves) took 25.7 s direct and 150.8 s with
/// multigrid PCG; a round bar of 10-node tetrahedra (116,040 unknowns, 2 modes, 40 solves)
/// took 38 s direct and 216 s with p-multigrid PCG.
const CROSSOVER: Crossover = Crossover::Flops {
    flops: f64::INFINITY,
    entries: elastic::DIRECT_ENTRIES,
    flat: true,
};

/// The most modes. Reason: each iteration solves for 2 vectors of each mode, so 100 modes cost
/// 200 solves an iteration; a user who needs more modes needs a code with Lanczos.
pub const MAX_MODES: usize = 100;

/// Solve the model on a laptop.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Summary, String> {
    solve_within(model, None, events)
}

/// Solve the model with the linear solver `method`, else the size of the model chooses it.
pub fn solve_within(
    model: &Model,
    method: Option<Method>,
    events: &mut dyn Events,
) -> Result<Summary, String> {
    let s = &model.structure;
    let stiffness = check(s)?;
    if model.count == 0 || model.count > MAX_MODES {
        return Err(format!(
            "the count of modes must be 1 to {MAX_MODES}, not {}",
            model.count
        ));
    }
    if s.symmetry.iter().any(|&v| v) {
        return Err(
            "a modes step solves the whole model: a half model has only the modes that mirror"
                .into(),
        );
    }
    if s.thermal != Thermal::None || !s.probes.is_empty() || !s.surfaces.is_empty() {
        return Err("a modes step has no thermal load, probe or surface".into());
    }
    for l in &s.loads {
        match l {
            Load::Displacement { value, .. } | Load::Point { value, .. }
                if value.iter().flatten().all(|&v| v == 0.0) => {}
            _ => {
                return Err(
                    "a modes step has only supports: displacement components held at 0".into(),
                )
            }
        }
    }
    for p in &s.parts {
        let m = &s.materials[p.material];
        if !(m.density > 0.0) {
            return Err(format!(
                "part {}: a modes step needs the density of material {}",
                p.name, m.name
            ));
        }
    }
    let method = method.or(s.method);
    let assembly = if s.parts.iter().any(|p| p.footprint.is_some()) {
        tet_assembly(s, &stiffness, events)?
    } else {
        grid_assembly(s, &stiffness, events)?
    };
    let Assembly {
        mesh,
        k,
        m,
        fixed,
        mass,
        solver,
    } = assembly;
    let n = mesh.points.len();
    let free: Vec<usize> = (0..3 * n).filter(|&i| !fixed[i]).collect();
    let (kf, mf) = (k.submatrix(&free), m.submatrix(&free));
    let (kd, md) = (kf.diagonal(), mf.diagonal());
    let mean = kd.iter().zip(&md).map(|(k, m)| k / m).sum::<f64>() / free.len() as f64;
    let shift = -SHIFT * mean;
    let mut a = kf.clone();
    for (v, w) in a.val.iter_mut().zip(&mf.val) {
        *v -= shift * w;
    }
    let solver = solver(a, &free, shift, method)?;
    let used = solver.method();
    events.progress(&format!(
        "{} unknowns, {} solver, shift {shift:.3e}",
        free.len(),
        match used {
            Method::Direct => heat::name(used),
            Method::Multigrid => "iterative (PCG with multigrid)",
        }
    ));
    let (mut solves, mut iterations) = (0, 0);
    let events_ref = &*events;
    let mut solve = |b: &[f64], x: &mut [f64]| {
        let stats = solver.solve(b, x, &|| events_ref.cancelled())?;
        solves += 1;
        iterations += stats.iterations;
        Ok(())
    };
    let cancelled = || events_ref.cancelled();
    let pairs = eigen::subspace(&kf, &mf, shift, model.count, &mut solve, &cancelled)?;
    if iterations > 0 {
        events.progress(&format!("PCG {iterations} iterations in {solves} solves"));
    }
    events.progress(&format!(
        "subspace iteration: {} iterations",
        pairs.iterations
    ));
    // The rigid motions have eigenvalues near 0 of either sign: the size of K_ii / M_ii times
    // the rounding of the solve.
    let zero = 1e-10 * mean;
    let mut mx = vec![0.0; free.len()];
    for (value, mut phi) in pairs.values.iter().zip(pairs.vectors) {
        let big = phi
            .iter()
            .fold(0.0f64, |a, &v| if v.abs() > a.abs() { v } else { a });
        if big < 0.0 {
            phi.iter_mut().for_each(|v| *v = -*v);
        }
        mf.mul(&phi, &mut mx);
        let mut participation = [0.0; 3];
        for (j, &i) in free.iter().enumerate() {
            participation[i % 3] += mx[j];
        }
        let mut shape = vec![[0.0; 3]; n];
        for (j, &i) in free.iter().enumerate() {
            shape[i / 3][i % 3] = phi[j];
        }
        let omega2 = if value.abs() <= zero { 0.0 } else { *value };
        events.mode(&Mode {
            frequency: omega2.max(0.0).sqrt() / (2.0 * std::f64::consts::PI),
            eigenvalue: *value,
            participation,
            mass_fraction: participation.map(|g| g * g / mass),
            shape,
        })?;
    }
    let mut warnings = Vec::new();
    if pairs.values.iter().any(|&v| v < -zero) {
        warnings.push(
            "a mode has an eigenvalue below 0: the stiffness is not positive semi-definite".into(),
        );
    }
    Ok(Summary {
        cells: mesh.len(),
        points: n,
        unknowns: free.len(),
        method: used,
        iterations: pairs.iterations,
        mass,
        warnings,
    })
}

/// The factory of the solver of K - shift M on the free unknowns.
type MakeSolver<'a> =
    Box<dyn FnOnce(Csr, &[usize], f64, Option<Method>) -> Result<Solver, String> + 'a>;

/// The matrices of a mesh: K and M of all unknowns, the fixed unknowns, the mass of the model
/// and the factory of the solver.
struct Assembly<'a> {
    mesh: Mesh,
    k: Csr,
    m: Csr,
    fixed: Vec<bool>,
    mass: f64,
    solver: MakeSolver<'a>,
}

/// The fixed unknowns of the supports of `s`: `face_points(set)` gives the points of a face
/// set, and `point(x)` the point of a point support.
fn supports(
    s: &elastic::Model,
    points: &[[f64; 3]],
    face_points: &dyn Fn(usize) -> Vec<usize>,
    point: &dyn Fn([f64; 3]) -> Result<usize, String>,
) -> Result<Vec<bool>, String> {
    let n = points.len();
    let mut fixed: Vec<Option<f64>> = vec![None; 3 * n];
    let mut owner = vec![u32::MAX; 3 * n];
    for (li, load) in s.loads.iter().enumerate() {
        match load {
            Load::Displacement { faces, value } => {
                for p in face_points(*faces) {
                    fix_node(&mut fixed, &mut owner, p, value, li, points[p])?;
                }
            }
            Load::Point { point: x, value } => {
                let p = point(*x)?;
                fix_node(&mut fixed, &mut owner, p, value, li, points[p])?;
            }
            _ => unreachable!("solve refuses the other loads"),
        }
    }
    Ok(fixed.iter().map(Option::is_some).collect())
}

/// The face sets of the supports.
fn used_faces(s: &elastic::Model) -> Vec<bool> {
    (0..s.faces.len())
        .map(|f| {
            s.loads
                .iter()
                .any(|l| matches!(l, Load::Displacement { faces, .. } if *faces == f))
        })
        .collect()
}

/// The matrices on the box grid.
fn grid_assembly<'a>(
    s: &'a elastic::Model,
    stiffness: &'a [hex::Stiffness],
    events: &mut dyn Events,
) -> Result<Assembly<'a>, String> {
    let keys = vec![Vec::new(); s.parts.len()];
    let (grid, faces, mids) = build_grid(
        &s.parts,
        s.spacing,
        [false; 3],
        &s.faces,
        &used_faces(s),
        &keys,
        "the materials of the parts",
    )?;
    let numbering = number(&grid, elastic::MAX_POINTS)?;
    let (mesh, ijk, cell_of) = (&numbering.mesh, &numbering.ijk, &numbering.cell_of);
    let n = ijk.len();
    let dims = grid.dims();
    events.mesh(mesh)?;
    events.progress(&format!(
        "grid {} x {} x {} planes, {} cells, {} points",
        dims[0] + 1,
        dims[1] + 1,
        dims[2] + 1,
        mesh.len(),
        n
    ));
    let elements: Vec<[u32; 24]> = mesh
        .cells
        .chunks_exact(8)
        .map(|c| std::array::from_fn(|k| 3 * c[k / 3] + (k % 3) as u32))
        .collect();
    let mut k = Csr::pattern(3 * n, elements.iter().map(|e| &e[..]));
    let mut m = k.clone();
    let mut mass = 0.0;
    for (e, &cell) in cell_of.iter().enumerate() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let mi = s.parts[grid.part[cell] as usize].material;
        let size = grid.size(cell);
        let rho = s.materials[mi].density;
        k.add_element(&elements[e], &Hex::new(size, &stiffness[mi], [0.0; 3]).k);
        m.add_element(&elements[e], &hex::mass(size, rho));
        mass += rho * size[0] * size[1] * size[2];
    }
    let node_of = |p: [usize; 3]| numbering.number[grid.point_index(p)] as usize;
    let fixed = supports(
        s,
        &mesh.points,
        &|set| {
            faces[set]
                .iter()
                .flat_map(|&f| grid.face_points(f))
                .map(node_of)
                .collect()
        },
        &|x| grid_point(&grid, &numbering, &mids, x),
    )?;
    let mesh = mesh.clone();
    let solver: MakeSolver = Box::new(move |a, free, shift, method| {
        let level = Level {
            planes: grid.planes.clone(),
            points: free.iter().map(|&i| numbering.ijk[i / 3]).collect(),
            comp: free.iter().map(|&i| (i % 3) as u8).collect(),
        };
        let element = |e: usize| {
            let cell = numbering.cell_of[e];
            let mi = s.parts[grid.part[cell] as usize].material;
            let size = grid.size(cell);
            let mut a = Hex::new(size, &stiffness[mi], [0.0; 3]).k;
            for (v, w) in a.iter_mut().zip(hex::mass(size, s.materials[mi].density)) {
                *v -= shift * w;
            }
            a
        };
        let cells = Cells {
            origin: numbering
                .cell_of
                .iter()
                .map(|&c| grid.cell_ijk(c).map(|v| v as u32))
                .collect(),
            element: &element,
        };
        Solver::new(a, Unknowns::Grid(level), method, CROSSOVER, Some(cells))
    });
    Ok(Assembly {
        mesh,
        k,
        m,
        fixed,
        mass,
        solver,
    })
}

/// The matrices on the extruded mesh of 10-node tetrahedra.
fn tet_assembly<'a>(
    s: &'a elastic::Model,
    stiffness: &[hex::Stiffness],
    events: &mut dyn Events,
) -> Result<Assembly<'a>, String> {
    let refusal = format!(
        "the mesh of 10-node tetrahedra has more than {TET_MAX_POINTS} points, the most that a \
         laptop solves in a few minutes: use a larger max_cell or a larger max_growth"
    );
    let tm = prism::mesh(&s.parts, s.axis, s.spacing, TET_MAX_POINTS, &refusal, false)?;
    let q = Quadratic::new(&tm.points, &tm.tets);
    if q.points.len() > TET_MAX_POINTS {
        return Err(refusal);
    }
    let n = q.points.len();
    let mesh = Mesh {
        points: q.points.clone(),
        kind: Cell::Tet10,
        cells: q.tets.iter().flatten().copied().collect(),
        part: tm.part.clone(),
    };
    events.mesh(&mesh)?;
    events.progress(&format!(
        "mesh {} triangles x {} planes, {} tetrahedra of 10 points, {} points",
        tm.triangles,
        tm.planes,
        mesh.len(),
        n
    ));
    let sets = select_tet_faces(&s.faces, &used_faces(s), &tm)?;
    let elements: Vec<[u32; 30]> = q
        .tets
        .iter()
        .map(|t| std::array::from_fn(|k| 3 * t[k / 3] + (k % 3) as u32))
        .collect();
    let mut k = Csr::pattern(3 * n, elements.iter().map(|e| &e[..]));
    let mut m = k.clone();
    let mut mass = 0.0;
    for (e, t) in tm.tets.iter().enumerate() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let mi = s.parts[tm.part[e] as usize].material;
        let rho = s.materials[mi].density;
        let el = Tet10::new(t.map(|v| tm.points[v as usize]));
        k.add_element(&elements[e], &el.stiffness(&stiffness[mi]));
        m.add_element(&elements[e], &el.mass(rho));
        mass += rho * el.volume;
    }
    let fixed = supports(
        s,
        &q.points,
        &|set| {
            sets[set]
                .iter()
                .flat_map(|&f| q.face(tm.faces[f].tet as usize, tm.faces[f].nodes))
                .map(|p| p as usize)
                .collect()
        },
        &|x| corner_at(&tm, x),
    )?;
    let solver: MakeSolver = Box::new(move |a, free, _, method| {
        let (p, coarse) = prolongation(&q.points, q.corners, &q.edges, free);
        Solver::new(
            a,
            Unknowns::Quadratic {
                points: free.iter().map(|&i| q.points[i / 3]).collect(),
                p,
                coarse,
            },
            method,
            CROSSOVER,
            None,
        )
    });
    Ok(Assembly {
        mesh,
        k,
        m,
        fixed,
        mass,
        solver,
    })
}
