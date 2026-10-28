//! Linear static thermo-elasticity on the box grid, with the 8-node hex of incompatible modes
//! (`hex`).
//!
//! Each point of the grid has 3 displacement unknowns. The materials are isotropic or
//! orthotropic with their axes on x, y and z, with an expansion coefficient for each axis. The
//! loads are fixed displacement components, a pressure and a traction on named faces, gravity
//! and a body force in a part, and a temperature change from the stress-free temperature: a
//! uniform temperature, or the field of a steady heat solve of the same parts (one-way
//! coupling: the stress does not change the heat flow). Parts that touch share their points, so
//! they are bonded.
//!
//! The results are the displacement, the strain and the stress at each point (the mean of the
//! values of the cells around it), the von Mises stress and the principal stresses, the reaction
//! of each fixed face set, the values at the probes, the largest von Mises stress of each part,
//! and the flatness and the curvature of named surfaces. The largest stress of a part is often
//! at a point where the exact stress is infinite: a re-entrant edge or corner, an edge where 2
//! materials meet a free face or each other, or the edge of a clamped face. There, the value
//! grows when the cells get smaller, and the result flags it.
//!
//! The force balance of each result, the applied loads plus the reactions, is 0 to the
//! precision of the linear solve.
//!
//! Sources: T. J. R. Hughes, The Finite Element Method, Prentice-Hall 1987, chapter 2 (linear
//! elasticity, the boundary conditions and the assembly); S. P. Timoshenko and J. N. Goodier,
//! Theory of Elasticity, 3rd ed., McGraw-Hill 1970, chapter 13 (thermal stress); O. C.
//! Zienkiewicz and J. Z. Zhu, The superconvergent patch recovery, Int. J. Numer. Meth. Engng 33
//! (1992) 1331-1364, section 1 (nodal averages of the stress); M. L. Williams, Stress
//! singularities resulting from various boundary conditions in angular corners of plates in
//! extension, J. Appl. Mech. 19 (1952) 526-528, and D. B. Bogy, Edge-bonded dissimilar
//! orthogonal elastic wedges under normal and shear loading, J. Appl. Mech. 35 (1968) 460-466
//! (the singular points); O. K. Smith, Eigenvalues of a symmetric 3 x 3 matrix, Comm. ACM 4
//! (1961) 168 (the principal stresses).

use crate::dc;
use crate::grid::{Face, Grid, Part, Side, Spacing, VOID};
use crate::heat::{
    self, build_grid, locate_probe, number, reduced_rhs, FaceSet, Mesh, Numbering, Probe,
};
use crate::hex::{shape, Hex, Orthotropic, Stiffness, BITS};
use crate::multigrid::{Cells, Level};
use crate::solve::{Crossover, Method, Solver, Unknowns, PCG_MAX_ITER};
use crate::sparse::Csr;

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub elastic: Orthotropic,
    /// The expansion coefficient along x, y and z.
    pub expansion: [f64; 3],
    /// The mass density, for gravity.
    pub density: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Load {
    /// Fixed displacement components on the faces of a face set. `None` leaves a component
    /// free.
    Displacement {
        faces: usize,
        value: [Option<f64>; 3],
    },
    /// Fixed displacement components at 1 point of the grid, to stop a rigid motion of a free
    /// body. A point support that takes a force gives a warning: the stress next to it depends
    /// on the mesh.
    Point {
        point: [f64; 3],
        value: [Option<f64>; 3],
    },
    /// A pressure on the faces of a face set: the force is -p n per area, n the outward normal.
    Pressure { faces: usize, value: f64 },
    /// A force per area on the faces of a face set.
    Traction { faces: usize, value: [f64; 3] },
    /// An acceleration of gravity on all parts: the force per volume is density times it.
    Gravity([f64; 3]),
    /// A force per volume in a part.
    BodyForce { part: usize, value: [f64; 3] },
}

/// The temperature of the thermal strain.
#[derive(Debug, Clone, PartialEq)]
pub enum Thermal {
    None,
    /// The same temperature at each point.
    Uniform(f64),
    /// The temperature of a steady heat solve. Its parts, spacing and symmetry must be those
    /// of the elastic model.
    Heat(Box<heat::Model>),
    /// The temperature of the Joule heat of a DC conduction with a heat solve. Its parts and
    /// spacing must be those of the elastic model, with no symmetry.
    Joule(Box<dc::Model>),
}

/// A surface whose shape the result gives: the faces of a face set, all with 1 outward normal.
#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    pub name: String,
    pub faces: usize,
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
    pub thermal: Thermal,
    /// The temperature with no thermal strain.
    pub reference: f64,
    pub probes: Vec<Probe>,
    pub surfaces: Vec<Surface>,
    /// The linear solver, else the cost of the factor and the shape of the cells choose it.
    pub method: Option<Method>,
}

/// The values at a probe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProbeValue {
    pub displacement: [f64; 3],
    /// xx, yy, zz, xy, yz, xz.
    pub stress: [f64; 6],
    pub mises: f64,
    pub temperature: f64,
}

/// The shape of a surface. `a` and `b` are the 2 axes of its plane (y and z, x and z, or x and
/// y), and w is the displacement along the outward normal.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceShape {
    pub name: String,
    /// The axis of the normal.
    pub axis: usize,
    /// The largest minus the smallest w.
    pub range: f64,
    /// The range of w minus the plane of the least squares fit of w.
    pub flatness: f64,
    /// The curvatures -d2w/da2, -d2w/db2 and the twist -d2w/dadb of the least squares fit of a
    /// quadratic to w. A curvature above 0 bulges outward.
    pub curvature: [f64; 3],
    /// The principal curvatures, the larger first.
    pub principal: [f64; 2],
}

impl SurfaceShape {
    /// `convex` (it bulges outward), `concave` or `flat`: the sign of the mean curvature.
    pub fn sign(&self) -> &'static str {
        let mean = self.principal[0] + self.principal[1];
        if mean > 0.0 {
            "convex"
        } else if mean < 0.0 {
            "concave"
        } else {
            "flat"
        }
    }
}

/// Why the exact stress at a point is infinite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Singular {
    /// A re-entrant edge or corner of a part.
    ReEntrant,
    /// An edge or a corner where 2 or more materials meet.
    MaterialEdge,
    /// The edge of a face set that holds 2 or 3 displacement components.
    SupportEdge,
}

impl Singular {
    pub fn describe(self) -> &'static str {
        match self {
            Singular::ReEntrant => "a re-entrant edge or corner",
            Singular::MaterialEdge => "an edge where 2 materials meet",
            Singular::SupportEdge => "the edge of a clamped face",
        }
    }
}

/// The largest von Mises stress of a part, at the corners of its cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Peak {
    pub part: usize,
    pub mises: f64,
    pub point: [f64; 3],
    /// The point is singular, or in a cell that touches a singular point: the value depends on
    /// the mesh.
    pub singular: Option<Singular>,
    /// The largest value at a point that is not singular, and the point.
    pub regular: Option<(f64, [f64; 3])>,
}

/// The reaction force of the components that a face set fixes. A component that 2 face sets fix
/// counts in the first. A plane of symmetry has no reaction.
#[derive(Debug, Clone, PartialEq)]
pub struct Reaction {
    pub name: String,
    pub force: [f64; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub displacement: Vec<[f64; 3]>,
    /// The temperature at each point, with a thermal load.
    pub temperature: Option<Vec<f64>>,
    /// The stress at each point: xx, yy, zz, xy, yz, xz.
    pub stress: Vec<[f64; 6]>,
    /// The total strain at each point, tensor components in the order of the stress.
    pub strain: Vec<[f64; 6]>,
    pub mises: Vec<f64>,
    /// The principal stresses at each point, the largest first.
    pub principal: Vec<[f64; 3]>,
    pub probes: Vec<ProbeValue>,
    pub surfaces: Vec<SurfaceShape>,
    pub peaks: Vec<Peak>,
    pub reactions: Vec<Reaction>,
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
    fn solution(&mut self, solution: &Solution) -> Result<(), String>;
}

/// The end of a solve.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub cells: usize,
    pub points: usize,
    pub unknowns: usize,
    pub method: Method,
    /// |applied loads + reactions| / the sum of the magnitudes of the applied loads, the thermal
    /// forces of the cells and the reactions, the largest of x, y and z.
    pub imbalance: f64,
    /// The things that the person must know about the result.
    pub warnings: Vec<String>,
}

/// A grid with more points than this needs more memory or time than a laptop has. Reason: the
/// ignored test `crossover` (Apple M-series, 1 thread) factored a thin plate of 361,800
/// unknowns in 10 s with 1.2e8 factor entries (1 GB). The factor of a thin model grows about
/// as n log n, so 300,000 points (900,000 unknowns) need about 3 GB and a few minutes.
const MAX_POINTS: heat::Limit = heat::Limit {
    points: 300_000,
    tet_points: 0,
    memory: "the memory of a laptop",
};

/// The solver is direct when its factor costs up to this many flops for each unknown. Reason:
/// in the test `crossover`, a cube of 9,450 unknowns (1.5e5 flops each) took 0.18 s direct and
/// 0.14 s multigrid, and one of 19,494 (3.3e5 flops each) 0.62 s against 0.31 s.
pub const DIRECT_FLOPS: f64 = 1.5e5;

/// Above this many entries, the factor is too large for a laptop, and the solver is multigrid also
/// on flat cells. Reason: the ignored test `crossover` factored a thin plate of 361,800 unknowns
/// with 1.2e8 entries (1 GB) in 10 s, and 2 GB of factor stays well inside the RAM of a 16 GB
/// laptop.
pub const DIRECT_ENTRIES: u64 = 250_000_000;

/// Above this ratio of the longest to the shortest side of a cell, the solver is direct up to
/// `DIRECT_ENTRIES`. Reason: in the test `crossover`, multigrid PCG took 10 iterations on cubes, 59 on plates
/// with cells of ratio 4, 290 to 381 at ratio 16 (8 times the direct time), and did not
/// converge in 500 on a panel with cells of ratio 40.
pub const MULTIGRID_ASPECT: f64 = 2.0;

/// A force imbalance above this fraction is a warning.
pub(crate) const IMBALANCE: f64 = 1e-6;

/// Solve the model on a laptop.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Summary, String> {
    solve_within(model, DIRECT_ENTRIES, PCG_MAX_ITER, events)
}

/// Solve the model, use multigrid when the direct factor has more than `entries` entries, and
/// stop multigrid after `iterations`.
pub fn solve_within(
    model: &Model,
    entries: u64,
    iterations: usize,
    events: &mut dyn Events,
) -> Result<Summary, String> {
    let stiffness = check(model)?;
    if model.parts.iter().any(|p| p.footprint.is_some()) {
        return crate::elastic_tet::solve(model, &stiffness, entries, iterations, events);
    }
    let mut warnings = Vec::new();
    let used: Vec<bool> = (0..model.faces.len())
        .map(|f| {
            model.surfaces.iter().any(|s| s.faces == f)
                || model.loads.iter().any(|l| match l {
                    Load::Displacement { faces, .. }
                    | Load::Pressure { faces, .. }
                    | Load::Traction { faces, .. } => *faces == f,
                    Load::Point { .. } | Load::Gravity(_) | Load::BodyForce { .. } => false,
                })
        })
        .collect();
    let mut body = vec![[0.0f64; 3]; model.parts.len()];
    for l in &model.loads {
        if let Load::BodyForce { part, value } = l {
            for d in 0..3 {
                body[*part][d] += value[d];
            }
        }
    }
    let keys: Vec<Vec<u64>> = body.iter().map(|b| b.map(f64::to_bits).to_vec()).collect();
    let (grid, faces, mids) = build_grid(
        &model.parts,
        model.spacing,
        model.symmetry,
        &model.faces,
        &used,
        &keys,
        "the materials or the body forces of the parts",
    )?;
    if mids.iter().any(Option::is_some) {
        let names: Vec<String> = (0..3)
            .filter_map(|d| mids[d].map(|m| format!("{} = {m}", ["x", "y", "z"][d])))
            .collect();
        warnings.push(format!(
            "the model is the part above the planes of symmetry {}: the reactions are those of \
             this part",
            names.join(" and ")
        ));
    }
    let numbering = number(&grid, MAX_POINTS)?;
    let (mesh, ijk, cell_of) = (&numbering.mesh, &numbering.ijk, &numbering.cell_of);
    let hexes: Vec<[u32; 8]> = mesh
        .cells
        .chunks_exact(8)
        .map(|c| c.try_into().expect("8 points"))
        .collect();
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
    let node_of = |p: [usize; 3]| numbering.number[grid.point_index(p)] as usize;

    // The temperature change at each point.
    let temperature = solve_thermal(&model.thermal, n, events, &mut warnings)?;
    let dt: Vec<f64> = match &temperature {
        Some(t) => t.iter().map(|t| t - model.reference).collect(),
        None => vec![0.0; n],
    };

    // The stiffness and the loads.
    let dofs = |nodes: &[u32; 8]| -> [u32; 24] {
        std::array::from_fn(|k| 3 * nodes[k / 3] + (k % 3) as u32)
    };
    let elements: Vec<[u32; 24]> = hexes.iter().map(dofs).collect();
    let mut k = Csr::pattern(3 * n, elements.iter().map(|e| &e[..]));
    let mut force = vec![0.0; 3 * n];
    let mut gravity = [0.0; 3];
    for l in &model.loads {
        if let Load::Gravity(g) = l {
            for d in 0..3 {
                gravity[d] += g[d];
            }
        }
    }
    // The scale of the force balance: the applied forces, the thermal forces of each cell (they
    // are in balance in each cell) and the reactions.
    let mut gross = 0.0;
    for (e, &cell) in cell_of.iter().enumerate() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let part = grid.part[cell] as usize;
        let m = &model.materials[model.parts[part].material];
        let size = grid.size(cell);
        let hex = Hex::new(size, &stiffness[model.parts[part].material], m.expansion);
        k.add_element(&elements[e], &hex.k);
        let nodes = &hexes[e];
        let t: [f64; 8] = nodes.map(|v| dt[v as usize]);
        let volume = size[0] * size[1] * size[2];
        for r in 0..24 {
            let mut f = 0.0;
            for c in 0..8 {
                f += hex.load[r * 8 + c] * t[c];
            }
            gross += f.abs();
            let d = r % 3;
            f += (m.density * gravity[d] + body[part][d]) * volume / 8.0;
            force[elements[e][r] as usize] += f;
        }
    }
    // The applied forces, without the thermal loads, for the force balance.
    let mut applied = [0.0; 3];
    for &cell in cell_of {
        let part = grid.part[cell] as usize;
        let m = &model.materials[model.parts[part].material];
        let s = grid.size(cell);
        for d in 0..3 {
            let f = (m.density * gravity[d] + body[part][d]) * s[0] * s[1] * s[2];
            applied[d] += f;
            gross += f.abs();
        }
    }
    let mut fixed: Vec<Option<f64>> = vec![None; 3 * n];
    // The first face set that fixes each unknown, for the reactions.
    let mut owner: Vec<u32> = vec![u32::MAX; 3 * n];
    let mut clamp: Vec<bool> = vec![false; n];
    let mut clamp_faces: Vec<Face> = Vec::new();
    // The point of each point support.
    let mut supports: Vec<(usize, usize)> = Vec::new();
    let mut fix = |node: usize, value: &[Option<f64>; 3], li: usize| {
        fix_node(&mut fixed, &mut owner, node, value, li, mesh.points[node])
    };
    for (li, load) in model.loads.iter().enumerate() {
        match load {
            Load::Displacement { faces: set, value } => {
                let held = value.iter().filter(|v| v.is_some()).count();
                for &face in &faces[*set] {
                    if held >= 2 {
                        clamp_faces.push(face);
                    }
                    for p in grid.face_points(face) {
                        let node = node_of(p);
                        if held >= 2 {
                            clamp[node] = true;
                        }
                        fix(node, value, li)?;
                    }
                }
            }
            Load::Point { point, value } => {
                let node = grid_point(&grid, &numbering, &mids, *point)?;
                supports.push((li, node));
                fix(node, value, li)?;
            }
            Load::Pressure { faces: set, value } => {
                for &face in &faces[*set] {
                    let side = Side::ALL[face.side as usize];
                    let mut t = [0.0; 3];
                    t[side.axis] = if side.positive { -value } else { *value };
                    surface_force(
                        &grid,
                        face,
                        t,
                        &node_of,
                        &mut force,
                        &mut applied,
                        &mut gross,
                    );
                }
            }
            Load::Traction { faces: set, value } => {
                for &face in &faces[*set] {
                    surface_force(
                        &grid,
                        face,
                        *value,
                        &node_of,
                        &mut force,
                        &mut applied,
                        &mut gross,
                    );
                }
            }
            Load::Gravity(_) | Load::BodyForce { .. } => {}
        }
    }
    // The planes of symmetry hold the normal component.
    for i in 0..n {
        for d in 0..3 {
            if mids[d].is_some() && ijk[i][d] == 0 {
                match fixed[3 * i + d] {
                    Some(v) if v != 0.0 => {
                        return Err(format!(
                            "a fixed u{} = {v} is on the plane of symmetry: it does not mirror. \
                             Solve the full model",
                            ["x", "y", "z"][d]
                        ))
                    }
                    _ => fixed[3 * i + d] = Some(0.0),
                }
            }
        }
    }
    check_supports(model, mesh, &fixed)?;

    let element_hex = |cell: usize| {
        let part = grid.part[cell] as usize;
        let mi = model.parts[part].material;
        Hex::new(
            grid.size(cell),
            &stiffness[mi],
            model.materials[mi].expansion,
        )
    };
    let free: Vec<usize> = (0..3 * n).filter(|&i| fixed[i].is_none()).collect();
    let mut u: Vec<f64> = fixed.iter().map(|v| v.unwrap_or(0.0)).collect();
    let level = Level {
        planes: grid.planes.clone(),
        points: free.iter().map(|&i| ijk[i / 3]).collect(),
        comp: free.iter().map(|&i| (i % 3) as u8).collect(),
    };
    let element = |e: usize| element_hex(cell_of[e]).k;
    let cells = Cells {
        origin: cell_of
            .iter()
            .map(|&c| grid.cell_ijk(c).map(|v| v as u32))
            .collect(),
        element: &element,
    };
    let (aspect, thin) = cell_of
        .iter()
        .map(|&c| {
            let ijk = grid.cell_ijk(c);
            let size: [f64; 3] =
                std::array::from_fn(|d| grid.planes[d][ijk[d] + 1] - grid.planes[d][ijk[d]]);
            let ratio = size.iter().fold(0.0_f64, |a, &b| a.max(b))
                / size.iter().fold(f64::MAX, |a, &b| a.min(b));
            (ratio, grid.part[c] as usize)
        })
        .fold((1.0, 0), |a, b| if b.0 > a.0 { b } else { a });
    let flat = aspect > MULTIGRID_ASPECT;
    let solver = Solver::new(
        k.submatrix(&free),
        Unknowns::Grid(level),
        model.method,
        Crossover::Flops {
            flops: DIRECT_FLOPS,
            entries,
            flat,
        },
        Some(cells),
    )?
    .with_max_iter(iterations);
    let method = solver.method();
    events.progress(
        &if flat && model.method.is_none() && method == Method::Multigrid {
            format!(
            "{} unknowns, multigrid solver: the direct factor needs more than {entries} entries",
            free.len()
        )
        } else {
            format!("{} unknowns, {} solver", free.len(), heat::name(method))
        },
    );
    let rhs = reduced_rhs(&k, &force, &u, &free, None);
    let mut x = vec![0.0; free.len()];
    let stats = solver
        .solve(&rhs, &mut x, &|| events.cancelled())
        .map_err(|e| {
            if flat && method == Method::Multigrid && e != "cancelled" {
                format!(
                    "{e}. The cells of part {} are {aspect:.0} times longer than they are thick, \
                     and multigrid converges slowly on flat cells. Put more cells through the \
                     thin layer (a smaller max_cell or a larger min_cells), or make the model \
                     smaller",
                    model.parts[thin].name
                )
            } else {
                e
            }
        })?;
    events.progress(&match stats.iterations {
        0 => format!("residual {:.1e}", stats.residual),
        i => format!("PCG {i} iterations, residual {:.1e}", stats.residual),
    });
    for (j, &i) in free.iter().enumerate() {
        u[i] = x[j];
    }

    let (reactions, imbalance) = reactions(
        model,
        &k,
        &u,
        &force,
        &Fixed {
            fixed: &fixed,
            owner: &owner,
            supports: &supports,
        },
        (applied, gross),
        &mesh.points,
        &mut warnings,
    );

    // The strain and the stress at the corners of each cell, the mean at each point, and the
    // peak of each part.
    let material_of: Vec<usize> = model.parts.iter().map(|p| p.material).collect();
    let singular = singular_points(&grid, &numbering, &material_of, &mids, &clamp, &clamp_faces);
    let mut stress = vec![[0.0; 6]; n];
    let mut strain = vec![[0.0; 6]; n];
    let mut count = vec![0u32; n];
    let mut peaks: Vec<Peak> = (0..model.parts.len())
        .map(|p| Peak {
            part: p,
            mises: f64::NEG_INFINITY,
            point: [0.0; 3],
            singular: None,
            regular: None,
        })
        .collect();
    for (e, &cell) in cell_of.iter().enumerate() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let hex = element_hex(cell);
        let nodes = &hexes[e];
        let ue: [f64; 24] = std::array::from_fn(|r| u[3 * nodes[r / 3] as usize + r % 3]);
        let te: [f64; 8] = nodes.map(|v| dt[v as usize]);
        let part = grid.part[cell] as usize;
        // The values of a cell that touches a singular point also depend on the mesh.
        let near = nodes.iter().find_map(|&v| singular[v as usize]);
        for (c, b) in BITS.iter().enumerate() {
            let xi = b.map(|v| if v == 1 { 1.0 } else { -1.0 });
            let (eps, sig) = hex.strain_stress(xi, &ue, &te);
            let (eps, sig) = (tensor_strain(&eps), reorder(&sig));
            let node = nodes[c] as usize;
            for i in 0..6 {
                stress[node][i] += sig[i];
                strain[node][i] += eps[i];
            }
            count[node] += 1;
            let vm = mises(&sig);
            let peak = &mut peaks[part];
            let x = mesh.points[node];
            let why = singular[node].or(near);
            if vm > peak.mises {
                peak.mises = vm;
                peak.point = x;
                peak.singular = why;
            }
            if why.is_none() && peak.regular.is_none_or(|(m, _)| vm > m) {
                peak.regular = Some((vm, x));
            }
        }
    }
    for i in 0..n {
        let c = f64::from(count[i].max(1));
        for j in 0..6 {
            stress[i][j] /= c;
            strain[i][j] /= c;
        }
    }
    let peaks: Vec<Peak> = peaks.into_iter().filter(|p| p.mises.is_finite()).collect();
    peak_warnings(model, &peaks, &mut warnings);

    let displacement: Vec<[f64; 3]> = (0..n)
        .map(|i| [u[3 * i], u[3 * i + 1], u[3 * i + 2]])
        .collect();
    let mut probes = Vec::new();
    for p in &model.probes {
        let (cell, l, mirrored) = locate_probe(&grid, &mids, p)?;
        let hex = element_hex(cell);
        let corners = grid.corners(cell);
        let nodes: [usize; 8] = corners.map(node_of);
        let ue: [f64; 24] = std::array::from_fn(|r| u[3 * nodes[r / 3] + r % 3]);
        let te: [f64; 8] = nodes.map(|v| dt[v]);
        let xi = l.map(|f| 2.0 * f - 1.0);
        let w = shape(xi);
        let mut d = [0.0; 3];
        for (c, &node) in nodes.iter().enumerate() {
            for a in 0..3 {
                d[a] += w[c] * displacement[node][a];
            }
        }
        let (_, sig) = hex.strain_stress(xi, &ue, &te);
        let mut sig = reorder(&sig);
        let t = match &temperature {
            Some(t) => nodes.iter().zip(w).map(|(&v, w)| w * t[v]).sum(),
            None => 0.0,
        };
        for a in 0..3 {
            if mirrored[a] {
                d[a] = -d[a];
                // The shear stresses with 1 index on the axis change sign: xy, yz, xz.
                for (s, pair) in [[0, 1], [1, 2], [0, 2]].iter().enumerate() {
                    if pair.contains(&a) {
                        sig[3 + s] = -sig[3 + s];
                    }
                }
            }
        }
        probes.push(ProbeValue {
            displacement: d,
            stress: sig,
            mises: mises(&sig),
            temperature: t,
        });
    }
    let surfaces = model
        .surfaces
        .iter()
        .map(|s| surface_shape(&grid, &faces[s.faces], &mids, &node_of, &displacement, s))
        .collect::<Result<Vec<_>, _>>()?;
    let mises_of: Vec<f64> = stress.iter().map(mises).collect();
    let principal: Vec<[f64; 3]> = stress.iter().map(principal).collect();
    events.solution(&Solution {
        displacement,
        temperature,
        stress,
        strain,
        mises: mises_of,
        principal,
        probes,
        surfaces,
        peaks,
        reactions,
    })?;
    Ok(Summary {
        cells: mesh.len(),
        points: n,
        unknowns: free.len(),
        method,
        imbalance,
        warnings,
    })
}

/// The fixed value of each unknown, the load that first fixes it, and the load and the point of
/// each point support.
pub(crate) struct Fixed<'a> {
    pub fixed: &'a [Option<f64>],
    pub owner: &'a [u32],
    pub supports: &'a [(usize, usize)],
}

/// Fix the components `value` of the point `node` (at `x`) for the load `li`. An error tells
/// that 2 loads give 2 values.
pub(crate) fn fix_node(
    fixed: &mut [Option<f64>],
    owner: &mut [u32],
    node: usize,
    value: &[Option<f64>; 3],
    li: usize,
    x: [f64; 3],
) -> Result<(), String> {
    for d in 0..3 {
        let Some(v) = value[d] else { continue };
        if owner[3 * node + d] == u32::MAX {
            owner[3 * node + d] = li as u32;
        }
        match fixed[3 * node + d] {
            Some(w) if w != v => {
                return Err(format!(
                    "2 fixed displacements give 2 values of u{} at the point ({:e}, {:e}, {:e}): \
                     {w} and {v}",
                    ["x", "y", "z"][d],
                    x[0],
                    x[1],
                    x[2]
                ))
            }
            _ => fixed[3 * node + d] = Some(v),
        }
    }
    Ok(())
}

/// The reactions K u - f at the fixed unknowns of each fixed face set, a warning for each point
/// support that takes a force, and the force imbalance. `scale` is the sum of the applied forces
/// and the sum of the magnitudes of the forces before the reactions.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reactions(
    model: &Model,
    k: &Csr,
    u: &[f64],
    force: &[f64],
    fixed: &Fixed,
    scale: ([f64; 3], f64),
    points: &[[f64; 3]],
    warnings: &mut Vec<String>,
) -> (Vec<Reaction>, f64) {
    let (applied, mut gross) = scale;
    let mut ku = vec![0.0; u.len()];
    k.mul(u, &mut ku);
    let mut reactions: Vec<Reaction> = Vec::new();
    let mut reaction_of = vec![usize::MAX; model.loads.len()];
    for (li, load) in model.loads.iter().enumerate() {
        if let Load::Displacement { faces, .. } = load {
            reaction_of[li] = reactions.len();
            reactions.push(Reaction {
                name: model.faces[*faces].name.clone(),
                force: [0.0; 3],
            });
        }
    }
    let mut total = applied;
    let mut point_force = vec![[0.0; 3]; model.loads.len()];
    for i in 0..u.len() {
        if fixed.fixed[i].is_some() {
            let r = ku[i] - force[i];
            total[i % 3] += r;
            gross += r.abs();
            let o = fixed.owner[i] as usize;
            match reaction_of.get(o) {
                Some(&j) if j != usize::MAX => reactions[j].force[i % 3] += r,
                Some(_) => point_force[o][i % 3] += r,
                None => {}
            }
        }
    }
    for &(li, node) in fixed.supports {
        let f = point_force[li];
        if f.iter().any(|v| v.abs() > IMBALANCE * gross) {
            let x = points[node];
            warnings.push(format!(
                "the point support at ({:e}, {:e}, {:e}) takes the force ({:.4e}, {:.4e}, \
                 {:.4e}): the stress next to it depends on the mesh. Hold the model on a face, \
                 or hold only a rigid motion at the point",
                x[0], x[1], x[2], f[0], f[1], f[2]
            ));
        }
    }
    let imbalance = if gross > 0.0 {
        total.iter().fold(0.0f64, |m, v| m.max(v.abs())) / gross
    } else {
        0.0
    };
    if imbalance > IMBALANCE {
        warnings.push(format!(
            "the force balance is off by {imbalance:.2e} of the forces: the linear solve is not \
             exact"
        ));
    }
    (reactions, imbalance)
}

/// A warning for each part whose largest von Mises stress is at or next to a singular point.
pub(crate) fn peak_warnings(model: &Model, peaks: &[Peak], warnings: &mut Vec<String>) {
    for p in peaks {
        if let Some(why) = p.singular {
            let regular = match p.regular {
                Some((m, x)) => format!(
                    "; the largest value away from such points is {m:.4e} at ({:.6e}, {:.6e}, \
                     {:.6e})",
                    x[0], x[1], x[2]
                ),
                None => String::new(),
            };
            warnings.push(format!(
                "part {}: the largest von Mises stress {:.4e} is at ({:.6e}, {:.6e}, {:.6e}), at or \
                 next to {}. The exact stress there is infinite, so the value grows when the cells \
                 get smaller{regular}",
                model.parts[p.part].name,
                p.mises,
                p.point[0],
                p.point[1],
                p.point[2],
                why.describe()
            ));
        }
    }
}

/// The temperature of the thermal load at each of the `n` points of the mesh (of the corners on
/// a mesh of tetrahedra).
pub(crate) fn solve_thermal(
    thermal: &Thermal,
    n: usize,
    events: &mut dyn Events,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<f64>>, String> {
    let (t, notes) = match thermal {
        Thermal::None => return Ok(None),
        Thermal::Uniform(t) => return Ok(Some(vec![*t; n])),
        Thermal::Heat(h) => {
            let mut run = HeatRun {
                outer: events,
                temperature: None,
            };
            let summary = heat::solve(h, &mut run)?;
            let t = run
                .temperature
                .ok_or("the heat solve gave no temperature")?;
            (t, summary.warnings)
        }
        Thermal::Joule(m) => {
            let s = dc::solve(m, &mut DcRun(events))?;
            let t = s
                .temperature
                .ok_or("the conduction solve gave no temperature")?;
            (t, s.warnings)
        }
    };
    for w in notes {
        warnings.push(format!("heat: {w}"));
    }
    if t.len() != n {
        return Err("the heat model has other parts or another mesh".into());
    }
    Ok(Some(t))
}

/// The progress of a conduction solve of a thermal load.
struct DcRun<'a, 'b>(&'a mut (dyn Events + 'b));

impl dc::Events for DcRun<'_, '_> {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }
    fn progress(&mut self, text: &str) {
        self.0.progress(&format!("conduction: {text}"));
    }
}

/// The heat solve of a thermal load: it keeps the temperature and passes the progress on.
pub(crate) struct HeatRun<'a> {
    pub outer: &'a mut dyn Events,
    pub temperature: Option<Vec<f64>>,
}

impl heat::Events for HeatRun<'_> {
    fn cancelled(&self) -> bool {
        self.outer.cancelled()
    }
    fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, text: &str) {
        self.outer.progress(&format!("heat: {text}"));
    }
    fn frame(&mut self, frame: &heat::Frame) -> Result<(), String> {
        self.temperature = Some(frame.temperature.clone());
        Ok(())
    }
}

/// Add a force per area `t` on a face: a quarter of the face force at each of its 4 points.
fn surface_force(
    grid: &Grid,
    face: Face,
    t: [f64; 3],
    node_of: &dyn Fn([usize; 3]) -> usize,
    force: &mut [f64],
    applied: &mut [f64; 3],
    gross: &mut f64,
) {
    let area = grid.face_area(face);
    for p in grid.face_points(face) {
        let node = node_of(p);
        for d in 0..3 {
            force[3 * node + d] += t[d] * area / 4.0;
        }
    }
    for d in 0..3 {
        applied[d] += t[d] * area;
        *gross += (t[d] * area).abs();
    }
}

/// The stress vector of `hex` (xx, yy, zz, yz, xz, xy) in the order of the output: xx, yy, zz,
/// xy, yz, xz.
pub(crate) fn reorder(s: &[f64; 6]) -> [f64; 6] {
    [s[0], s[1], s[2], s[5], s[3], s[4]]
}

/// The engineering strain of `hex` as tensor components in the order of the output.
pub(crate) fn tensor_strain(e: &[f64; 6]) -> [f64; 6] {
    [e[0], e[1], e[2], 0.5 * e[5], 0.5 * e[3], 0.5 * e[4]]
}

/// The von Mises stress of xx, yy, zz, xy, yz, xz.
pub fn mises(s: &[f64; 6]) -> f64 {
    let [xx, yy, zz, xy, yz, xz] = *s;
    (0.5 * ((xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2))
        + 3.0 * (xy * xy + yz * yz + xz * xz))
        .sqrt()
}

/// The principal stresses of xx, yy, zz, xy, yz, xz, the largest first (Smith 1961).
pub fn principal(s: &[f64; 6]) -> [f64; 3] {
    let [xx, yy, zz, xy, yz, xz] = *s;
    let q = (xx + yy + zz) / 3.0;
    let p1 = xy * xy + yz * yz + xz * xz;
    let p2 = (xx - q).powi(2) + (yy - q).powi(2) + (zz - q).powi(2) + 2.0 * p1;
    let p = (p2 / 6.0).sqrt();
    if p == 0.0 {
        return [q; 3];
    }
    // B = (A - q I) / p; r = det(B) / 2.
    let (a, b, c) = ((xx - q) / p, (yy - q) / p, (zz - q) / p);
    let (d, e, f) = (xy / p, yz / p, xz / p);
    let det = a * (b * c - e * e) - d * (d * c - e * f) + f * (d * e - b * f);
    let phi = (det / 2.0).clamp(-1.0, 1.0).acos() / 3.0;
    let s1 = q + 2.0 * p * phi.cos();
    let s3 = q + 2.0 * p * (phi + 2.0 * std::f64::consts::PI / 3.0).cos();
    [s1, 3.0 * q - s1 - s3, s3]
}

/// The singular points of the mesh: re-entrant edges and corners, edges where materials meet,
/// and the edges of the faces in `clamp_faces`. The 8 cells around a grid point decide the
/// first 2. The point is regular when their material (or void) changes along 1 axis at most: a
/// point inside a part, on a flat face or on a flat interface. Else, 2 or more materials make a
/// material edge, and a material whose cells there are not a box makes a re-entrant edge or
/// corner. A convex edge or corner of 1 material is regular. Beyond a plane of symmetry, the cells are the
/// mirrors of the cells inside.
fn singular_points(
    grid: &Grid,
    numbering: &heat::Numbering,
    material_of: &[usize],
    mids: &[Option<f64>; 3],
    clamp: &[bool],
    clamp_faces: &[Face],
) -> Vec<Option<Singular>> {
    let dims = grid.dims();
    let label = |c: [i64; 3]| -> u32 {
        let mut q = [0usize; 3];
        for d in 0..3 {
            let mut v = c[d];
            if v < 0 && mids[d].is_some() {
                v = -1 - v;
            }
            if v < 0 || v >= dims[d] as i64 {
                return 0;
            }
            q[d] = v as usize;
        }
        match grid.part[grid.cell_index(q)] {
            VOID => 0,
            p => material_of[p as usize] as u32 + 1,
        }
    };
    let mut out: Vec<Option<Singular>> = numbering
        .ijk
        .iter()
        .map(|p| {
            let mut l = [0u32; 8];
            for (k, b) in BITS.iter().enumerate() {
                l[k] = label(std::array::from_fn(|d| p[d] as i64 - 1 + b[d] as i64));
            }
            let at = |b: [usize; 3]| l[BITS.iter().position(|x| *x == b).expect("a corner")];
            let varies = (0..3)
                .filter(|&d| {
                    BITS.iter().any(|b| {
                        let mut o = *b;
                        o[d] = 1 - o[d];
                        at(*b) != at(o)
                    })
                })
                .count();
            if varies <= 1 {
                return None;
            }
            let mut parts: Vec<u32> = l.iter().copied().filter(|&v| v != 0).collect();
            parts.sort_unstable();
            parts.dedup();
            if parts.len() >= 2 {
                return Some(Singular::MaterialEdge);
            }
            // The solid cells are a box when they are the product of their index sets.
            let solid: Vec<[usize; 3]> = BITS.iter().copied().filter(|b| at(*b) != 0).collect();
            let span: usize = (0..3)
                .map(|d| {
                    let lo = solid.iter().map(|b| b[d]).min().unwrap_or(0);
                    let hi = solid.iter().map(|b| b[d]).max().unwrap_or(0);
                    hi - lo + 1
                })
                .product();
            (span != solid.len()).then_some(Singular::ReEntrant)
        })
        .collect();
    // A clamped point on a free face: an outer face that is not clamped and not on a plane of
    // symmetry.
    let mut clamped = clamp_faces.to_vec();
    clamped.sort_unstable();
    for face in grid.faces() {
        let side = Side::ALL[face.side as usize];
        let c = grid.cell_ijk(face.cell);
        if mids[side.axis].is_some() && !side.positive && c[side.axis] == 0 {
            continue;
        }
        if clamped.binary_search(&face).is_ok() {
            continue;
        }
        for p in grid.face_points(face) {
            let node = numbering.number[grid.point_index(p)] as usize;
            if clamp[node] && out[node].is_none() {
                out[node] = Some(Singular::SupportEdge);
            }
        }
    }
    out
}

/// The shape of a surface from the displacement along its normal, with the mirror of each face
/// across each plane of symmetry in the plane of the surface. The fits (`fit_shape`) use a 2 x 2
/// Gauss rule on each face.
fn surface_shape(
    grid: &Grid,
    faces: &[Face],
    mids: &[Option<f64>; 3],
    node_of: &dyn Fn([usize; 3]) -> usize,
    u: &[[f64; 3]],
    surface: &Surface,
) -> Result<SurfaceShape, String> {
    let side = Side::ALL[faces[0].side as usize];
    if faces.iter().any(|f| f.side != faces[0].side) {
        return Err(format!(
            "surface {}: its faces must all face 1 direction, for example z+: give its face set \
             a `side`",
            surface.name
        ));
    }
    let axis = side.axis;
    let sign = if side.positive { 1.0 } else { -1.0 };
    let (ia, ib) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    // Each face as its 4 corners: (a, b, w), with the mirrors.
    let mut quads: Vec<[[f64; 3]; 4]> = Vec::new();
    for &face in faces {
        let pts = grid.face_points(face);
        let q: [[f64; 3]; 4] = pts.map(|p| {
            let x = grid.point(p);
            [x[ia], x[ib], sign * u[node_of(p)][axis]]
        });
        quads.push(q);
    }
    // The mirrors: each face once for each subset of the planes of symmetry in the surface.
    let mut all = quads;
    for (k, d) in [ia, ib].into_iter().enumerate() {
        if let Some(m) = mids[d] {
            let copies: Vec<[[f64; 3]; 4]> = all
                .iter()
                .map(|q| {
                    q.map(|mut c| {
                        c[k] = 2.0 * m - c[k];
                        c
                    })
                })
                .collect();
            all.extend(copies);
        }
    }
    // The 2 x 2 Gauss points of each face, with the bilinear w of the face, and the corners.
    let g = 0.5 / 3f64.sqrt();
    let mut samples = Vec::new();
    for q in &all {
        let da = q[1][0] - q[0][0];
        let db = q[3][1] - q[0][1];
        let weight = (da * db).abs() / 4.0;
        for (fa, fb) in [
            (0.5 - g, 0.5 - g),
            (0.5 + g, 0.5 - g),
            (0.5 + g, 0.5 + g),
            (0.5 - g, 0.5 + g),
        ] {
            let n = [
                (1.0 - fa) * (1.0 - fb),
                fa * (1.0 - fb),
                fa * fb,
                (1.0 - fa) * fb,
            ];
            let w: f64 = (0..4).map(|k| n[k] * q[k][2]).sum();
            samples.push([q[0][0] + fa * da, q[0][1] + fb * db, w, weight]);
        }
    }
    let nodes: Vec<[f64; 3]> = all.iter().flatten().copied().collect();
    Ok(fit_shape(&surface.name, axis, &samples, &nodes))
}

/// The shape of a surface whose normal is the axis `axis`, from the displacement w along its
/// outward normal: the least squares fits of a plane and of a quadratic to the samples (a, b, w,
/// weight) of w over the area, and the range of w and of w minus the plane at the points
/// `nodes` (a, b, w).
pub(crate) fn fit_shape(
    name: &str,
    axis: usize,
    samples: &[[f64; 4]],
    nodes: &[[f64; 3]],
) -> SurfaceShape {
    // The centre and the half size, for the scale of the fit.
    let (mut area, mut ca, mut cb) = (0.0, 0.0, 0.0);
    for s in samples {
        area += s[3];
        ca += s[3] * s[0];
        cb += s[3] * s[1];
    }
    let (ca, cb) = (ca / area, cb / area);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for c in nodes {
        for k in 0..2 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    let half = (0.5 * (hi[0] - lo[0]).max(hi[1] - lo[1])).max(f64::MIN_POSITIVE);
    // The normal equations of w = sum c_k phi_k over the samples, with phi = 1, s, t, s^2, s t,
    // t^2 and s = (a - ca) / half, t = (b - cb) / half.
    let basis = |a: f64, b: f64| {
        let (s, t) = ((a - ca) / half, (b - cb) / half);
        [1.0, s, t, s * s, s * t, t * t]
    };
    let mut m = [[0.0; 6]; 6];
    let mut r = [0.0; 6];
    for &[a, b, w, weight] in samples {
        let phi = basis(a, b);
        for i in 0..6 {
            r[i] += weight * phi[i] * w;
            for j in 0..6 {
                m[i][j] += weight * phi[i] * phi[j];
            }
        }
    }
    let plane = solve_small(&m, &r, 3);
    let quad = solve_small(&m, &r, 6);
    let (mut wmin, mut wmax, mut rmin, mut rmax) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for c in nodes {
        let phi = basis(c[0], c[1]);
        let fit: f64 = (0..3).map(|k| plane[k] * phi[k]).sum();
        wmin = wmin.min(c[2]);
        wmax = wmax.max(c[2]);
        rmin = rmin.min(c[2] - fit);
        rmax = rmax.max(c[2] - fit);
    }
    let h2 = half * half;
    let curvature = [-2.0 * quad[3] / h2, -2.0 * quad[5] / h2, -quad[4] / h2];
    let mean = 0.5 * (curvature[0] + curvature[1]);
    let radius =
        (0.25 * (curvature[0] - curvature[1]).powi(2) + curvature[2] * curvature[2]).sqrt();
    SurfaceShape {
        name: name.to_owned(),
        axis,
        range: wmax - wmin,
        flatness: rmax - rmin,
        curvature,
        principal: [mean + radius, mean - radius],
    }
}

/// Solve the first `k` rows and columns of the symmetric system m x = r by Gauss elimination
/// with partial pivoting. A singular system (a surface on 1 line) gives 0 for the free terms.
pub(crate) fn solve_small(m: &[[f64; 6]; 6], r: &[f64; 6], k: usize) -> [f64; 6] {
    let mut a = [[0.0; 7]; 6];
    for i in 0..k {
        a[i][..k].copy_from_slice(&m[i][..k]);
        a[i][6] = r[i];
    }
    let scale = (0..k).map(|i| m[i][i].abs()).fold(0.0, f64::max);
    let mut x = [0.0; 6];
    let mut pivot_of = [usize::MAX; 6];
    let mut row = 0;
    for col in 0..k {
        let p = (row..k).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()));
        let Some(p) = p else { break };
        if a[p][col].abs() <= 1e-12 * scale {
            continue;
        }
        a.swap(row, p);
        for i in 0..k {
            if i != row {
                let f = a[i][col] / a[row][col];
                for j in 0..7 {
                    a[i][j] -= f * a[row][j];
                }
            }
        }
        pivot_of[col] = row;
        row += 1;
    }
    for col in 0..k {
        if pivot_of[col] != usize::MAX {
            let r = pivot_of[col];
            x[col] = a[r][6] / a[r][col];
        }
    }
    x
}

/// Each connected piece of the mesh needs fixed components that stop its 3 translations and 3
/// rotations, else its displacement has no unique value.
pub(crate) fn check_supports(
    model: &Model,
    mesh: &Mesh,
    fixed: &[Option<f64>],
) -> Result<(), String> {
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
    // The centre and the size of each piece, for the scale of the rotations.
    let mut lo = vec![[f64::INFINITY; 3]; n];
    let mut hi = vec![[f64::NEG_INFINITY; 3]; n];
    for i in 0..n {
        let r = find(&mut root, i);
        for d in 0..3 {
            lo[r][d] = lo[r][d].min(mesh.points[i][d]);
            hi[r][d] = hi[r][d].max(mesh.points[i][d]);
        }
    }
    // Sum r r^T over the fixed components, r the values of the 6 rigid motions there.
    let mut gram = vec![[[0.0f64; 6]; 6]; n];
    for i in 0..n {
        let p = find(&mut root, i);
        let size = (0..3)
            .map(|d| hi[p][d] - lo[p][d])
            .fold(0.0, f64::max)
            .max(f64::MIN_POSITIVE);
        let x: [f64; 3] =
            std::array::from_fn(|d| (mesh.points[i][d] - 0.5 * (lo[p][d] + hi[p][d])) / size);
        for d in 0..3 {
            if fixed[3 * i + d].is_none() {
                continue;
            }
            // Component d of the translations and of the rotations about x, y and z.
            let mut r = [0.0; 6];
            r[d] = 1.0;
            let rot = [[0.0, -x[2], x[1]], [x[2], 0.0, -x[0]], [-x[1], x[0], 0.0]];
            for a in 0..3 {
                r[3 + a] = rot[a][d];
            }
            for a in 0..6 {
                for b in 0..6 {
                    gram[p][a][b] += r[a] * r[b];
                }
            }
        }
    }
    let mut checked = vec![false; n];
    for e in 0..mesh.len() {
        let p = find(&mut root, mesh.cell(e)[0] as usize);
        if checked[p] {
            continue;
        }
        checked[p] = true;
        if let Some(motion) = free_motion(&gram[p]) {
            let part = &model.parts[mesh.part[e] as usize].name;
            return Err(format!(
                "part {part} and the parts that it touches can {motion} with no strain, so the \
                 displacement has no unique value: fix more displacement components (ux, uy or \
                 uz) on a face set"
            ));
        }
    }
    Ok(())
}

/// The name of a rigid motion that the fixed components do not stop, from their Gram matrix:
/// the Cholesky factor has a pivot of 0.
fn free_motion(g: &[[f64; 6]; 6]) -> Option<&'static str> {
    const NAMES: [&str; 6] = [
        "move along x",
        "move along y",
        "move along z",
        "turn about x",
        "turn about y",
        "turn about z",
    ];
    let scale = (0..6).map(|i| g[i][i]).fold(0.0, f64::max);
    if scale == 0.0 {
        return Some("move");
    }
    let mut l = [[0.0; 6]; 6];
    for j in 0..6 {
        let mut d = g[j][j];
        for k in 0..j {
            d -= l[j][k] * l[j][k];
        }
        if !(d > 1e-9 * scale) {
            return Some(NAMES[j]);
        }
        let d = d.sqrt();
        l[j][j] = d;
        for i in j + 1..6 {
            let mut v = g[i][j];
            for k in 0..j {
                v -= l[i][k] * l[j][k];
            }
            l[i][j] = v / d;
        }
    }
    None
}

/// The grid point at `x`, after the mirror into the solved part at each plane of symmetry.
fn grid_point(
    grid: &Grid,
    numbering: &Numbering,
    mids: &[Option<f64>; 3],
    x: [f64; 3],
) -> Result<usize, String> {
    let mut y = x;
    for d in 0..3 {
        if let Some(m) = mids[d] {
            if y[d] < m {
                y[d] = 2.0 * m - y[d];
            }
        }
    }
    let span = (0..3)
        .map(|d| grid.planes[d][grid.planes[d].len() - 1] - grid.planes[d][0])
        .fold(0.0, f64::max);
    let near: [usize; 3] = std::array::from_fn(|d| {
        let p = &grid.planes[d];
        (0..p.len())
            .min_by(|&a, &b| (p[a] - y[d]).abs().total_cmp(&(p[b] - y[d]).abs()))
            .unwrap_or(0)
    });
    let at: [f64; 3] = std::array::from_fn(|d| grid.planes[d][near[d]]);
    let node = numbering.number[grid.point_index(near)];
    if node == u32::MAX || (0..3).any(|d| (at[d] - y[d]).abs() > 1e-9 * span) {
        return Err(format!(
            "the point support at ({:e}, {:e}, {:e}) is not a point of the grid of the parts. A \
             point of the grid is a corner of a part, or a point where the planes of 2 parts \
             cross",
            x[0], x[1], x[2]
        ));
    }
    Ok(node as usize)
}

/// The checks of the model that the grid does not need, and the stiffness of each material.
fn check(model: &Model) -> Result<Vec<Stiffness>, String> {
    let stiffness = model
        .materials
        .iter()
        .map(|m| {
            m.elastic
                .stiffness()
                .map_err(|e| format!("material {}: {e}", m.name))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let prism = model.parts.iter().any(|p| p.footprint.is_some());
    if prism && model.symmetry.iter().any(|&s| s) {
        return Err(
            "symmetry needs a model of boxes: a model with a polygon or a circle part is solved \
             in full"
                .into(),
        );
    }
    if !prism {
        if let Some(f) = model.faces.iter().find(|f| f.circle.is_some()) {
            return Err(format!(
                "face set {}: `circle` selects the faces on a circle outline, and no part has one",
                f.name
            ));
        }
    }
    if model.axis > 2 {
        return Err("the extrusion axis must be x, y or z".into());
    }
    for m in &model.materials {
        if !m.expansion.iter().all(|a| a.is_finite()) {
            return Err(format!("material {}: the expansion must be finite", m.name));
        }
        if !(m.density.is_finite() && m.density >= 0.0) {
            return Err(format!(
                "material {}: the density must be 0 or more",
                m.name
            ));
        }
    }
    let finite = |v: &[f64]| v.iter().all(|x| x.is_finite());
    for load in &model.loads {
        let ok = match load {
            Load::Displacement { value, .. } => {
                value.iter().any(Option::is_some) && value.iter().flatten().all(|v| v.is_finite())
            }
            Load::Point { point, value } => {
                finite(point)
                    && value.iter().any(Option::is_some)
                    && value.iter().flatten().all(|v| v.is_finite())
            }
            Load::Pressure { value, .. } => value.is_finite(),
            Load::Traction { value, .. } | Load::Gravity(value) | Load::BodyForce { value, .. } => {
                finite(value)
            }
        };
        if !ok {
            return Err("each load needs finite numbers".into());
        }
        // A load mirrors at a plane of symmetry when it has no component along its normal.
        let along: Option<[f64; 3]> = match load {
            Load::Traction { value, .. } | Load::Gravity(value) | Load::BodyForce { value, .. } => {
                Some(*value)
            }
            Load::Displacement { value, .. } | Load::Point { value, .. } => {
                Some(value.map(|v| v.unwrap_or(0.0)))
            }
            Load::Pressure { .. } => None,
        };
        if let Some(v) = along {
            for d in 0..3 {
                if model.symmetry[d] && v[d] != 0.0 {
                    return Err(format!(
                        "symmetry in {0}: a load with a component along {0} does not mirror. \
                         Solve the full model",
                        ["x", "y", "z"][d]
                    ));
                }
            }
        }
    }
    if model.loads.iter().any(|l| matches!(l, Load::Gravity(_))) {
        for p in &model.parts {
            let m = &model.materials[p.material];
            if !(m.density > 0.0) {
                return Err(format!(
                    "part {}: gravity needs the density of material {}",
                    p.name, m.name
                ));
            }
        }
    }
    match &model.thermal {
        Thermal::None => {}
        Thermal::Uniform(t) => {
            if !(t.is_finite() && model.reference.is_finite()) {
                return Err("the temperature and the reference temperature must be finite".into());
            }
        }
        Thermal::Heat(h) => {
            if h.parts != model.parts
                || h.spacing != model.spacing
                || h.symmetry != model.symmetry
                || h.axis != model.axis
            {
                return Err("the heat model must have the parts and the grid of the model".into());
            }
            if h.step != heat::Step::Steady {
                return Err(
                    "the temperature of a static step comes from a steady heat solve".into(),
                );
            }
            if !model.reference.is_finite() {
                return Err("the reference temperature must be finite".into());
            }
        }
        Thermal::Joule(m) => {
            if m.parts != model.parts
                || m.spacing != model.spacing
                || m.axis != model.axis
                || model.symmetry != [false; 3]
            {
                return Err(
                    "the conduction model must have the parts and the grid of the model, with no \
                     symmetry"
                        .into(),
                );
            }
            if m.heat.is_none() {
                return Err("the Joule heat of a static step needs heat loads".into());
            }
            if !model.reference.is_finite() {
                return Err("the reference temperature must be finite".into());
            }
        }
    }
    Ok(stiffness)
}
