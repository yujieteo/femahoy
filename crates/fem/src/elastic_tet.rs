//! Linear static thermo-elasticity on the 2.5D tetrahedral mesh (`prism`) with 10-node
//! tetrahedra (`tet10`), for a model with a part that is a prism of a polygon or a circle.
//!
//! The model, the loads, the results and the checks are those of the box grid (`elastic`). The
//! mesh is the linear mesh of the heat solve with a point at the middle of each edge, so the
//! temperature of a steady heat solve of the same parts is at the corners, and it is linear in
//! each cell. A face load goes to the 3 edge points of each 6-point face with its consistent
//! weights, and the pressure on a face is along the normal of that face, also on a face of a
//! circle outline.
//!
//! The result at each point is the mean of the values of the cells around it, at that point. A
//! probe gives the mean of the cells that hold it: inside a cell the value of that cell, and on
//! a face, an edge or a corner the mean of the cells that meet there.
//!
//! A singular point is a corner of the mesh where the solid or an interface has a kink: on the
//! outer faces, the part of the solid around the point is not convex (a re-entrant edge or
//! corner); 2 materials meet at an outer face or at a kink of their interface (a material
//! edge); or a clamped point is on a free face (the edge of a clamped face). A kink is an angle
//! above 30 degrees between 2 faces at the point, so the 64 or more segments of a circle are
//! smooth.
//!
//! Sources: T. J. R. Hughes, The Finite Element Method, Prentice-Hall 1987, chapter 2 (linear
//! elasticity, the boundary conditions and the assembly); O. C. Zienkiewicz, R. L. Taylor and
//! J. Z. Zhu, The Finite Element Method: Its Basis and Fundamentals, 7th ed., Elsevier 2013,
//! chapter 6 (the quadratic tetrahedron); O. C. Zienkiewicz and J. Z. Zhu, The superconvergent
//! patch recovery, Int. J. Numer. Meth. Engng 33 (1992) 1331-1364, section 1 (nodal averages of
//! the stress); M. L. Williams, Stress singularities resulting from various boundary conditions
//! in angular corners of plates in extension, J. Appl. Mech. 19 (1952) 526-528 (the singular
//! points).

use std::collections::HashMap;

use crate::elastic::{
    check_supports, fit_shape, fix_node, mises, peak_warnings, principal, reactions, reorder,
    solve_thermal, tensor_strain, Events, Fixed, Load, Model, Peak, ProbeValue, Singular, Solution,
    Summary, Surface, SurfaceShape,
};
use crate::heat::{self, face_normal, select_tet_faces, tet_gradients, Cell, Mesh};
use crate::hex::Stiffness;
use crate::pmultigrid::prolongation;
use crate::prism::{self, across, TetMesh};
use crate::solve::{Crossover, Method, Solver, Unknowns};
use crate::sparse::Csr;
use crate::tet10::{node_coordinates, shape, Quadratic, Tet10};

/// A quadratic mesh with more points than this needs more memory or time than a laptop has.
/// Reason: a thick tube of 183,000 points (536,797 unknowns) took 18 PCG iterations, 53 s and a
/// peak of 2.3 GB on a 16 GB laptop that ran other work, and one of about 390,000 points went
/// past 3.2 GB and stalled in memory compression.
pub const MAX_POINTS: usize = 250_000;

/// Up to this many unknowns, the solver on cells that are not flat is direct. Reason: in the test
/// `crossover`, a ring of 39,037 unknowns took 0.99 s direct and 0.71 s with p-multigrid, and one
/// of 142,535 took 42.7 s against 3.2 s. The flop count of the factor is not a test here: its
/// analysis took 15 s of a 73 s run of 536,797 unknowns.
pub const DIRECT_UNKNOWNS: usize = 30_000;

/// Above this ratio of the width of a cell across the extrusion axis to its height along it, the
/// solver is direct up to `DIRECT_ENTRIES`. Reason: in the test `crossover`, on a plate with a
/// hole under its weight, PCG with p-multigrid took 2.5 s against 4.6 s direct at ratio 4.6, 59 s
/// against 68 s at ratio 9.2, and did not converge in 500 iterations at ratio 18, 23 and 46: the
/// linear tetrahedra of the coarse level lock in bending.
pub const MULTIGRID_ASPECT: f64 = 8.0;

/// The cosine of the angle between 2 faces at a point above which the faces make no kink: 30
/// degrees. Reason: the segments of a circle meet at 5.6 degrees or less (64 or more segments),
/// and a polygon corner of 30 degrees or less has a stress that is almost regular (the exponent
/// of Williams is above 0.9).
const SMOOTH: f64 = 0.866;

/// The error of a mesh with more points than `limit.tet_points`.
pub(crate) fn refusal(limit: heat::Limit) -> String {
    let why = if limit.tet_points < MAX_POINTS {
        format!("{} solves", limit.memory)
    } else {
        "a laptop solves in a few minutes".to_string()
    };
    format!(
        "the mesh of 10-node tetrahedra has more than {} points, the most that {why}: use a \
         larger max_cell or a larger max_growth",
        limit.tet_points
    )
}

/// Solve the model on the tetrahedral mesh: the direct solver on a small model or on flat cells,
/// else PCG with p-multigrid, also when the direct factor has more than `entries` entries; at
/// most `iterations`. A mesh with more points than `limit.tet_points` is refused.
pub(crate) fn solve(
    model: &Model,
    stiffness: &[Stiffness],
    limit: heat::Limit,
    entries: u64,
    iterations: usize,
    events: &mut dyn Events,
) -> Result<Summary, String> {
    let mut warnings = Vec::new();
    let refusal = refusal(limit);
    let tm = prism::mesh(
        &model.parts,
        model.axis,
        model.spacing,
        limit.tet_points,
        &refusal,
        false,
    )?;
    let q = Quadratic::new(&tm.points, &tm.tets);
    if q.points.len() > limit.tet_points {
        return Err(refusal);
    }
    let (nv, n) = (q.corners, q.points.len());
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

    // The temperature at each corner.
    let temperature = solve_thermal(&model.thermal, nv, events, &mut warnings)?;
    let dt: Vec<f64> = match &temperature {
        Some(t) => t.iter().map(|t| t - model.reference).collect(),
        None => vec![0.0; nv],
    };
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
    let sets = select_tet_faces(&model.faces, &used, &tm)?;

    // The stiffness and the loads.
    let mut body = vec![[0.0f64; 3]; model.parts.len()];
    let mut gravity = [0.0; 3];
    for l in &model.loads {
        match l {
            Load::BodyForce { part, value } => {
                for d in 0..3 {
                    body[*part][d] += value[d];
                }
            }
            Load::Gravity(g) => {
                for d in 0..3 {
                    gravity[d] += g[d];
                }
            }
            _ => {}
        }
    }
    // D alpha of each material.
    let thermal_of: Vec<[f64; 3]> = model
        .materials
        .iter()
        .zip(stiffness)
        .map(|(m, d)| {
            std::array::from_fn(|i| (0..3).map(|j| d.normal[i][j] * m.expansion[j]).sum())
        })
        .collect();
    let material = |e: usize| model.parts[tm.part[e] as usize].material;
    let tets: Vec<Tet10> = tm
        .tets
        .iter()
        .map(|t| Tet10::new(t.map(|v| tm.points[v as usize])))
        .collect();
    let elements: Vec<[u32; 30]> = q
        .tets
        .iter()
        .map(|t| std::array::from_fn(|k| 3 * t[k / 3] + (k % 3) as u32))
        .collect();
    let mut k = Csr::pattern(3 * n, elements.iter().map(|e| &e[..]));
    let mut force = vec![0.0; 3 * n];
    let mut applied = [0.0; 3];
    // The scale of the force balance, as on the box grid.
    let mut gross = 0.0;
    for (e, el) in tets.iter().enumerate() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let mi = material(e);
        k.add_element(&elements[e], &el.stiffness(&stiffness[mi]));
        if temperature.is_some() {
            let t = tm.tets[e].map(|v| dt[v as usize]);
            for (r, f) in el.thermal_load(thermal_of[mi], t).iter().enumerate() {
                force[elements[e][r] as usize] += f;
                gross += f.abs();
            }
        }
        let part = tm.part[e] as usize;
        let b: [f64; 3] =
            std::array::from_fn(|d| model.materials[mi].density * gravity[d] + body[part][d]);
        if b != [0.0; 3] {
            for (i, &node) in q.tets[e].iter().enumerate() {
                let w = if i < 4 {
                    -el.volume / 20.0
                } else {
                    el.volume / 5.0
                };
                for d in 0..3 {
                    force[3 * node as usize + d] += b[d] * w;
                }
            }
            for d in 0..3 {
                applied[d] += b[d] * el.volume;
                gross += (b[d] * el.volume).abs();
            }
        }
    }
    let face_points = |f: usize| q.face(tm.faces[f].tet as usize, tm.faces[f].nodes);
    let corners_of = |f: usize| tm.faces[f].nodes.map(|v| tm.points[v as usize]);
    let mut fixed: Vec<Option<f64>> = vec![None; 3 * n];
    let mut owner: Vec<u32> = vec![u32::MAX; 3 * n];
    let mut clamp = vec![false; n];
    let mut clamp_faces = vec![false; tm.faces.len()];
    let mut supports: Vec<(usize, usize)> = Vec::new();
    for (li, load) in model.loads.iter().enumerate() {
        let t = match load {
            Load::Displacement { faces: set, value } => {
                let held = value.iter().filter(|v| v.is_some()).count();
                for &f in &sets[*set] {
                    clamp_faces[f] |= held >= 2;
                    for p in face_points(f) {
                        let p = p as usize;
                        clamp[p] |= held >= 2;
                        fix_node(&mut fixed, &mut owner, p, value, li, q.points[p])?;
                    }
                }
                continue;
            }
            Load::Point { point, value } => {
                let node = corner_at(&tm, *point)?;
                supports.push((li, node));
                fix_node(&mut fixed, &mut owner, node, value, li, q.points[node])?;
                continue;
            }
            Load::Pressure { faces: set, value } => (*set, None, *value),
            Load::Traction { faces: set, value } => (*set, Some(*value), 0.0),
            Load::Gravity(_) | Load::BodyForce { .. } => continue,
        };
        let (set, traction, pressure) = t;
        for &f in &sets[set] {
            let (normal, area) = face_normal(corners_of(f));
            let t = traction.unwrap_or(normal.map(|v| -pressure * v));
            let pts = face_points(f);
            for &p in &pts[3..] {
                for d in 0..3 {
                    force[3 * p as usize + d] += t[d] * area / 3.0;
                }
            }
            for d in 0..3 {
                applied[d] += t[d] * area;
                gross += (t[d] * area).abs();
            }
        }
    }
    check_supports(model, &mesh, &fixed)?;

    // The flattest cell: its longest edge across the axis over its height along the axis.
    let (aspect, thin) = tm
        .tets
        .iter()
        .zip(&tm.part)
        .map(|(t, &part)| {
            let p = t.map(|v| tm.points[v as usize]);
            let along = |k: usize| p[k][model.axis];
            let height = (0..4).map(along).fold(f64::NEG_INFINITY, f64::max)
                - (0..4).map(along).fold(f64::INFINITY, f64::min);
            let across = (0..4)
                .flat_map(|i| (i + 1..4).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let (a, b) = (across(model.axis, p[i]), across(model.axis, p[j]));
                    (a[0] - b[0]).hypot(a[1] - b[1])
                })
                .fold(0.0, f64::max);
            (across / height, part as usize)
        })
        .fold((0.0, 0), |a, b| if b.0 > a.0 { b } else { a });
    let flat = aspect > MULTIGRID_ASPECT;
    let free: Vec<usize> = (0..3 * n).filter(|&i| fixed[i].is_none()).collect();
    let mut u: Vec<f64> = fixed.iter().map(|v| v.unwrap_or(0.0)).collect();
    let (p, coarse) = prolongation(&q.points, nv, &q.edges, &free);
    let solver = Solver::new(
        k.submatrix(&free),
        Unknowns::Quadratic {
            points: free.iter().map(|&i| q.points[i / 3]).collect(),
            p,
            coarse,
        },
        model.method,
        if flat {
            Crossover::Flops {
                flops: f64::INFINITY,
                entries,
                flat,
            }
        } else {
            Crossover::Unknowns(DIRECT_UNKNOWNS)
        },
        None,
    )?
    .with_max_iter(iterations);
    let method = solver.method();
    events.progress(&format!(
        "{} unknowns, {} solver{}",
        free.len(),
        match method {
            Method::Direct => heat::name(method),
            Method::Multigrid => "iterative (PCG with p-multigrid)",
        },
        if flat && model.method.is_none() && method == Method::Multigrid {
            format!(": the direct factor needs more than {entries} entries")
        } else {
            String::new()
        }
    ));
    let rhs = heat::reduced_rhs(&k, &force, &u, &free, None);
    let mut x = vec![0.0; free.len()];
    let stats = solver
        .solve(&rhs, &mut x, &|| events.cancelled())
        .map_err(|e| {
            if flat && method == Method::Multigrid && e != "cancelled" {
                format!(
                    "{e}. The cells of part {} are {aspect:.0} times wider than they are high \
                     along the axis, and p-multigrid converges slowly on flat cells. Put more \
                     cells through the thin layer (a smaller max_cell or a larger min_cells), or \
                     make the model smaller",
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
        &q.points,
        &mut warnings,
    );

    // The strain and the stress at the 10 points of each cell, the mean at each point, and the
    // peak of each part.
    let material_of: Vec<usize> = model.parts.iter().map(|p| p.material).collect();
    let singular = singular_points(&tm, &q, &material_of, &clamp, &clamp_faces);
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
    let at = |e: usize, l: [f64; 4]| -> ([f64; 6], [f64; 6]) {
        let mi = material(e);
        let ue: [f64; 30] = std::array::from_fn(|r| u[3 * q.tets[e][r / 3] as usize + r % 3]);
        let te = tm.tets[e].map(|v| dt[v as usize]);
        let (eps, sig) = tets[e].strain_stress(&stiffness[mi], thermal_of[mi], l, &ue, te);
        (tensor_strain(&eps), reorder(&sig))
    };
    for e in 0..tets.len() {
        if e % 4096 == 0 && events.cancelled() {
            return Err("cancelled".into());
        }
        let nodes = &q.tets[e];
        let near = nodes[..4].iter().find_map(|&v| singular[v as usize]);
        let part = tm.part[e] as usize;
        for (c, l) in node_coordinates().into_iter().enumerate() {
            let (eps, sig) = at(e, l);
            let node = nodes[c] as usize;
            for i in 0..6 {
                stress[node][i] += sig[i];
                strain[node][i] += eps[i];
            }
            count[node] += 1;
            let vm = mises(&sig);
            let peak = &mut peaks[part];
            let x = q.points[node];
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
        let hits = cells_at(&tm, p.point);
        if hits.is_empty() {
            return Err(format!(
                "probe {} at ({}, {}, {}) is in no part",
                p.name, p.point[0], p.point[1], p.point[2]
            ));
        }
        let share = 1.0 / hits.len() as f64;
        let mut value = ProbeValue {
            displacement: [0.0; 3],
            stress: [0.0; 6],
            mises: 0.0,
            temperature: 0.0,
        };
        for (e, l) in hits {
            let w = shape(l);
            for (c, &node) in q.tets[e].iter().enumerate() {
                for a in 0..3 {
                    value.displacement[a] += share * w[c] * displacement[node as usize][a];
                }
            }
            let (_, sig) = at(e, l);
            for i in 0..6 {
                value.stress[i] += share * sig[i];
            }
            if let Some(t) = &temperature {
                value.temperature += share
                    * (0..4)
                        .map(|c| l[c] * t[tm.tets[e][c] as usize])
                        .sum::<f64>();
            }
        }
        value.mises = mises(&value.stress);
        probes.push(value);
    }
    let surfaces = model
        .surfaces
        .iter()
        .map(|s| surface_shape(&q, &tm, &sets[s.faces], &displacement, s))
        .collect::<Result<Vec<_>, _>>()?;
    let temperature = temperature.map(|t| {
        let middles: Vec<f64> = q
            .edges
            .iter()
            .map(|[a, b]| 0.5 * (t[*a as usize] + t[*b as usize]))
            .collect();
        [t, middles].concat()
    });
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

/// The corner of the mesh at `x`, for a point support.
pub(crate) fn corner_at(tm: &TetMesh, x: [f64; 3]) -> Result<usize, String> {
    let (lo, hi) = bounds(&tm.points);
    let span = (0..3).map(|d| hi[d] - lo[d]).fold(0.0, f64::max);
    let dist = |p: &[f64; 3]| (0..3).map(|d| (p[d] - x[d]).abs()).fold(0.0, f64::max);
    let near = (0..tm.points.len())
        .min_by(|&a, &b| dist(&tm.points[a]).total_cmp(&dist(&tm.points[b])))
        .ok_or("the mesh has no point")?;
    if dist(&tm.points[near]) > 1e-9 * span {
        return Err(format!(
            "the point support at ({:e}, {:e}, {:e}) is not a corner of the mesh. A corner of the \
             mesh is a corner of a polygon or of a box, or a point of a circle at 0, 90, 180 or 270 \
             degrees from its centre, on a plane at an end of a part along the axis",
            x[0], x[1], x[2]
        ));
    }
    Ok(near)
}

fn bounds(points: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in points {
        for d in 0..3 {
            lo[d] = lo[d].min(p[d]);
            hi[d] = hi[d].max(p[d]);
        }
    }
    (lo, hi)
}

/// The cells that hold the point `x`, each with the volume coordinates of `x` in it.
fn cells_at(tm: &TetMesh, x: [f64; 3]) -> Vec<(usize, [f64; 4])> {
    let mut out = Vec::new();
    for (e, tet) in tm.tets.iter().enumerate() {
        let p = tet.map(|i| tm.points[i as usize]);
        let (g, _) = tet_gradients(p);
        let mut l = [0.0; 4];
        for i in 1..4 {
            l[i] = (0..3).map(|d| g[i][d] * (x[d] - p[0][d])).sum();
        }
        l[0] = 1.0 - l[1] - l[2] - l[3];
        if l.iter().all(|&v| v >= -1e-9) {
            out.push((e, l));
        }
    }
    out
}

/// The singular points of the mesh (see the module text). The edge points are not singular:
/// the cells that touch a singular corner flag them.
fn singular_points(
    tm: &TetMesh,
    q: &Quadratic,
    material_of: &[usize],
    clamp: &[bool],
    clamp_faces: &[bool],
) -> Vec<Option<Singular>> {
    let nv = tm.points.len();
    // The material of each corner, and whether 2 or more meet there.
    let mut first = vec![usize::MAX; nv];
    let mut many = vec![false; nv];
    for (tet, &part) in tm.tets.iter().zip(&tm.part) {
        let m = material_of[part as usize];
        for &v in tet {
            let v = v as usize;
            if first[v] == usize::MAX {
                first[v] = m;
            } else if first[v] != m {
                many[v] = true;
            }
        }
    }
    // The faces between 2 materials.
    let mut seen: HashMap<[u32; 3], usize> = HashMap::new();
    let mut interfaces: Vec<[u32; 3]> = Vec::new();
    for (tet, &part) in tm.tets.iter().zip(&tm.part) {
        let m = material_of[part as usize];
        for f in [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]] {
            let nodes = f.map(|k| tet[k]);
            let mut key = nodes;
            key.sort_unstable();
            match seen.remove(&key) {
                Some(other) if other != m => interfaces.push(nodes),
                Some(_) => {}
                None => {
                    seen.insert(key, m);
                }
            }
        }
    }
    let mut outer: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for (k, f) in tm.faces.iter().enumerate() {
        for &v in &f.nodes {
            outer[v as usize].push(k as u32);
        }
    }
    let mut inner: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for (k, f) in interfaces.iter().enumerate() {
        for &v in f {
            inner[v as usize].push(k as u32);
        }
    }
    let corners = |nodes: &[u32; 3]| nodes.map(|v| tm.points[v as usize]);
    let mut out = vec![None; q.points.len()];
    for v in 0..nv {
        let x = tm.points[v];
        if many[v] {
            let normals: Vec<[f64; 3]> = inner[v]
                .iter()
                .map(|&k| face_normal(corners(&interfaces[k as usize])).0)
                .collect();
            let kink = normals.iter().enumerate().any(|(i, a)| {
                normals[i + 1..]
                    .iter()
                    .any(|b| (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).abs() < SMOOTH)
            });
            if !outer[v].is_empty() || kink {
                out[v] = Some(Singular::MaterialEdge);
            }
            continue;
        }
        // Re-entrant: a corner of an outer face at the point lies in front of the plane of
        // another outer face at the point by more than half its distance (30 degrees).
        let re_entrant = outer[v].iter().any(|&f| {
            let (normal, _) = face_normal(corners(&tm.faces[f as usize].nodes));
            outer[v].iter().any(|&g| {
                tm.faces[g as usize].nodes.iter().any(|&w| {
                    let d: [f64; 3] = std::array::from_fn(|k| tm.points[w as usize][k] - x[k]);
                    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    d[0] * normal[0] + d[1] * normal[1] + d[2] * normal[2] > 0.5 * len
                })
            })
        });
        if re_entrant {
            out[v] = Some(Singular::ReEntrant);
        }
    }
    for (k, f) in tm.faces.iter().enumerate() {
        if clamp_faces[k] {
            continue;
        }
        for node in q.face(f.tet as usize, f.nodes) {
            let node = node as usize;
            if clamp[node] && out[node].is_none() {
                out[node] = Some(Singular::SupportEdge);
            }
        }
    }
    out
}

/// The shape of a surface from the displacement along its normal: the fits of `fit_shape` with
/// the 3 edge points of each 6-point face, each with the weight of a third of its area (the
/// rule of degree 2 on a triangle), and the range over the 6 points of each face.
fn surface_shape(
    q: &Quadratic,
    tm: &TetMesh,
    faces: &[usize],
    u: &[[f64; 3]],
    surface: &Surface,
) -> Result<SurfaceShape, String> {
    let normal_of = |f: usize| face_normal(tm.faces[f].nodes.map(|v| tm.points[v as usize]));
    let (first, _) = normal_of(faces[0]);
    let axis = (0..3)
        .max_by(|&a, &b| first[a].abs().total_cmp(&first[b].abs()))
        .expect("3 axes");
    let sign = first[axis].signum();
    let one_way = faces
        .iter()
        .all(|&f| normal_of(f).0[axis] * sign > 1.0 - 1e-9);
    if !one_way {
        return Err(format!(
            "surface {}: its faces must all face 1 direction, for example z+: give its face set \
             a `side`",
            surface.name
        ));
    }
    let (ia, ib) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut samples = Vec::new();
    let mut nodes = Vec::new();
    for &f in faces {
        let (_, area) = normal_of(f);
        let pts = q.face(tm.faces[f].tet as usize, tm.faces[f].nodes);
        for (k, &p) in pts.iter().enumerate() {
            let x = q.points[p as usize];
            let c = [x[ia], x[ib], sign * u[p as usize][axis]];
            nodes.push(c);
            if k >= 3 {
                samples.push([c[0], c[1], c[2], area / 3.0]);
            }
        }
    }
    Ok(fit_shape(&surface.name, axis, &samples, &nodes))
}
