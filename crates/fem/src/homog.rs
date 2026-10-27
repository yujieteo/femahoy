//! The homogenized (effective) material of a periodic unit cell: the conductivity tensor, the
//! stiffness tensor and the expansion coefficients of a material whose structure repeats the
//! cell in x, y and z.
//!
//! The cell is the bounding box of its parts. The field in the cell is a mean part (a unit mean
//! gradient of the temperature, or a unit mean strain) plus a periodic fluctuation: the 2 points
//! of each pair on opposite sides of the cell share 1 unknown of the fluctuation. The pairs come
//! from the coordinates of the points, so the box grid and the periodic extruded mesh (`prism`
//! with `periodic`) both give them. A point on 1 side with no partner on the opposite side is an
//! error: the cell is not periodic. 1 point of the fluctuation is fixed, because a constant
//! fluctuation changes nothing.
//!
//! - Conductivity: 3 solves, each with a unit mean gradient along x, y or z. The tensor is
//!   K*_ij = (1 / V) T_i^T K T_j, with T_i the mean part plus the fluctuation of solve i at the
//!   points of each cell: the mean of the heat flux under the unit gradients, by the energy.
//! - Stiffness: 6 solves, each with a unit mean strain (xx, yy, zz, yz, xz, xy, with the
//!   engineering shear strains). C*_ij = (1 / V) u_i^T K u_j, in the same way.
//! - Expansion: 1 solve with a unit temperature change and a mean strain of 0. Its mean stress is
//!   sigma_j = (1 / V) u_j^T (K w - f), from Hill's lemma (the mean of the product of a periodic
//!   stress and a periodic strain is the product of the means), with w the fluctuation and f the
//!   thermal load. Then alpha* = -C*^-1 sigma.
//!
//! The density is the mean over the cell volume, void included. The specific heat is the mean
//! weighted by the mass.
//!
//! The solver is direct: the pairs couple the 2 sides of the cell, so the grid levels of
//! multigrid do not hold. A unit cell is small, so 1 factor serves the 3, 6 and 1 solves.
//!
//! Sources: A. Bensoussan, J.-L. Lions and G. Papanicolaou, Asymptotic Analysis for Periodic
//! Structures, North-Holland 1978, chapter 1 (the cell problem and the effective tensor);
//! R. Hill, Elastic properties of reinforced solids: some theoretical principles, J. Mech. Phys.
//! Solids 11 (1963) 357-372 (the means of the stress and the strain, and Hill's lemma);
//! P. Suquet, Elements of homogenization for inelastic solid mechanics, in Homogenization
//! Techniques for Composite Media, Lecture Notes in Physics 272, Springer 1987, 193-278
//! (periodic conditions, the energy form of the effective stiffness, and the thermal strain);
//! J. C. Michel, H. Moulinec and P. Suquet, Effective properties of composite materials with
//! periodic microstructure: a computational approach, Comput. Methods Appl. Mech. Engrg. 172
//! (1999) 109-143 (periodic conditions as pairs of points on a finite element mesh).

use std::collections::HashMap;

use crate::cholesky::Cholesky;
use crate::elastic::DIRECT_ENTRIES;
use crate::grid::{Grid, Part, Spacing};
use crate::heat::{conduction, number, tet_gradients, Cell, Mesh, LAPTOP};
use crate::hex::{Hex, Orthotropic, Stiffness};
use crate::order::nested_dissection;
use crate::prism;
use crate::sparse::Csr;
use crate::symbolic::Symbolic;

/// A material of the cell. A property of the cell needs its key in each material of a part.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    /// The conductivity along x, y and z.
    pub conductivity: Option<[f64; 3]>,
    pub elastic: Option<Orthotropic>,
    /// The expansion coefficient along x, y and z.
    pub expansion: [f64; 3],
    pub density: Option<f64>,
    pub specific_heat: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub spacing: Spacing,
    /// The extrusion axis of the parts with a footprint (0 x, 1 y, 2 z).
    pub axis: usize,
}

/// The effective material of the cell. The vectors and the matrices of the stiffness are in the
/// order xx, yy, zz, yz, xz, xy, with the engineering shear strains.
#[derive(Debug, Clone, PartialEq)]
pub struct Effective {
    /// The corners of the cell.
    pub lo: [f64; 3],
    pub hi: [f64; 3],
    /// The volume fraction of each material. The rest of the cell is void.
    pub fraction: Vec<f64>,
    pub conductivity: Option<[[f64; 3]; 3]>,
    pub stiffness: Option<[[f64; 6]; 6]>,
    /// The thermal strain of a unit temperature change.
    pub expansion: Option<[f64; 6]>,
    pub density: Option<f64>,
    pub specific_heat: Option<f64>,
    /// The mesh of the cell.
    pub mesh: Mesh,
    pub unknowns: usize,
}

pub trait Events {
    /// True stops the solve.
    fn cancelled(&self) -> bool {
        false
    }
    /// A line of progress for the person.
    fn progress(&mut self, text: &str);
}

/// Solve the cell problems of the model.
pub fn solve(model: &Model, events: &mut dyn Events) -> Result<Effective, String> {
    let used: Vec<usize> = {
        let mut u: Vec<usize> = model.parts.iter().map(|p| p.material).collect();
        u.sort_unstable();
        u.dedup();
        u
    };
    let all = |has: &dyn Fn(&Material) -> bool, key: &str| -> Result<bool, String> {
        let with: Vec<&str> = used
            .iter()
            .map(|&m| &model.materials[m])
            .filter(|m| has(m))
            .map(|m| m.name.as_str())
            .collect();
        match with.len() {
            0 => Ok(false),
            n if n == used.len() => Ok(true),
            _ => {
                let without = used
                    .iter()
                    .map(|&m| &model.materials[m])
                    .find(|m| !has(m))
                    .expect("a material without the key");
                Err(format!(
                    "material {}: missing {key}. A property of the cell needs it in each \
                     material of a part (material {} has it)",
                    without.name, with[0]
                ))
            }
        }
    };
    let heat = all(&|m| m.conductivity.is_some(), "conductivity")?;
    let elastic = all(&|m| m.elastic.is_some(), "the elastic constants")?;
    let density = all(&|m| m.density.is_some(), "density")?;
    let specific = all(&|m| m.specific_heat.is_some(), "specific_heat")?;
    if !heat && !elastic {
        return Err(
            "the cell needs the conductivity or the elastic constants (youngs_modulus and \
             poisson_ratio) of its materials"
                .into(),
        );
    }
    let mut stiffness: Vec<Option<Stiffness>> = Vec::new();
    for m in &model.materials {
        if let Some(k) = m.conductivity {
            if k.iter().any(|k| !(k.is_finite() && *k > 0.0)) {
                return Err(format!(
                    "material {}: the conductivity must be above 0",
                    m.name
                ));
            }
        }
        if m.expansion.iter().any(|a| !a.is_finite()) {
            return Err(format!("material {}: the expansion must be finite", m.name));
        }
        for (v, key) in [(m.density, "density"), (m.specific_heat, "specific_heat")] {
            if v.is_some_and(|v| !(v.is_finite() && v >= 0.0)) {
                return Err(format!("material {}: {key} must be 0 or more", m.name));
            }
        }
        stiffness.push(match &m.elastic {
            Some(e) => Some(
                e.stiffness()
                    .map_err(|e| format!("material {}: {e}", m.name))?,
            ),
            None => None,
        });
    }
    if model.axis > 2 {
        return Err("the extrusion axis must be x, y or z".into());
    }
    let tets = model.parts.iter().any(|p| p.footprint.is_some());
    if tets && elastic {
        return Err(
            "the stiffness of a cell with a polygon or a circle part needs elasticity on \
             tetrahedra, which the grid solver does not have: give the parts as boxes, or \
             remove the elastic constants to get only the conductivity"
                .into(),
        );
    }
    // The mesh, and the side lengths of each box cell.
    let (mesh, sizes) = if tets {
        let refusal = format!(
            "the mesh has more than {} points, the most that the direct solver solves in a few \
             minutes: use a larger max_cell or a larger max_growth",
            LAPTOP.tet_points
        );
        let tm = prism::mesh(
            &model.parts,
            model.axis,
            model.spacing,
            LAPTOP.tet_points,
            &refusal,
            true,
        )?;
        let mesh = Mesh {
            points: tm.points,
            kind: Cell::Tet4,
            cells: tm.tets.iter().flatten().copied().collect(),
            part: tm.part,
        };
        (mesh, Vec::new())
    } else {
        let grid = Grid::build(&model.parts, model.spacing, [&[], &[], &[]])?;
        let numbering = number(&grid, LAPTOP)?;
        let sizes: Vec<[f64; 3]> = numbering.cell_of.iter().map(|&c| grid.size(c)).collect();
        (numbering.mesh, sizes)
    };
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for x in &mesh.points {
        for d in 0..3 {
            lo[d] = lo[d].min(x[d]);
            hi[d] = hi[d].max(x[d]);
        }
    }
    let volume: f64 = (0..3).map(|d| hi[d] - lo[d]).product();
    events.progress(&format!(
        "unit cell {} x {} x {}, {} {}, {} points",
        hi[0] - lo[0],
        hi[1] - lo[1],
        hi[2] - lo[2],
        mesh.len(),
        if tets { "tetrahedra" } else { "cells" },
        mesh.points.len()
    ));
    let master = pairs(&mesh, lo, hi)?;
    let masters = master.iter().copied().max().map_or(0, |m| m as usize + 1);
    // The volume of each cell, and the gradients of each tetrahedron.
    let nodes = mesh.kind.nodes();
    let mut cell_volume = Vec::with_capacity(mesh.len());
    let mut grads: Vec<[[f64; 3]; 4]> = Vec::new();
    for e in 0..mesh.len() {
        if tets {
            let c = mesh.cell(e);
            let (g, v) = tet_gradients(std::array::from_fn(|k| mesh.points[c[k] as usize]));
            grads.push(g);
            cell_volume.push(v);
        } else {
            cell_volume.push(sizes[e].iter().product());
        }
    }
    let material_of = |e: usize| model.parts[mesh.part[e] as usize].material;
    let mut fraction = vec![0.0; model.materials.len()];
    let mut mass = 0.0;
    let mut heat_mass = 0.0;
    for e in 0..mesh.len() {
        let m = &model.materials[material_of(e)];
        fraction[material_of(e)] += cell_volume[e] / volume;
        let rho = m.density.unwrap_or(0.0);
        mass += rho * cell_volume[e];
        heat_mass += rho * m.specific_heat.unwrap_or(0.0) * cell_volume[e];
    }
    let mut out = Effective {
        lo,
        hi,
        fraction,
        conductivity: None,
        stiffness: None,
        expansion: None,
        density: density.then_some(mass / volume),
        specific_heat: (density && specific && mass > 0.0).then_some(heat_mass / mass),
        mesh: Mesh {
            points: Vec::new(),
            kind: mesh.kind,
            cells: Vec::new(),
            part: Vec::new(),
        },
        unknowns: 0,
    };
    // The point of each corner of a cell, relative to the low corner of the cell.
    let rel = |p: u32| -> [f64; 3] {
        let x = mesh.points[p as usize];
        std::array::from_fn(|d| x[d] - lo[d])
    };
    if heat {
        let mut cache: HashMap<([u64; 3], usize), Vec<f64>> = HashMap::new();
        let element = |e: usize| -> Vec<f64> {
            let k = model.materials[material_of(e)]
                .conductivity
                .expect("a conductivity");
            if tets {
                let g = &grads[e];
                let v = cell_volume[e];
                let mut ke = vec![0.0; 16];
                for i in 0..4 {
                    for j in 0..4 {
                        ke[i * 4 + j] = v * (0..3).map(|d| k[d] * g[i][d] * g[j][d]).sum::<f64>();
                    }
                }
                ke
            } else {
                conduction(sizes[e], k).to_vec()
            }
        };
        // Each box cell of 1 size and 1 material has the same matrix.
        let mut matrix = |e: usize| -> Vec<f64> {
            if tets {
                return element(e);
            }
            cache
                .entry((sizes[e].map(f64::to_bits), material_of(e)))
                .or_insert_with(|| element(e))
                .clone()
        };
        let elements: Vec<Vec<u32>> = (0..mesh.len())
            .map(|e| mesh.cell(e).iter().map(|&p| master[p as usize]).collect())
            .collect();
        let mut k = Csr::pattern(masters, elements.iter().map(|e| &e[..]));
        let mut rhs = vec![vec![0.0; masters]; 3];
        for e in 0..mesh.len() {
            let ke = matrix(e);
            let d = &elements[e];
            k.add_element(d, &ke);
            let x: Vec<[f64; 3]> = mesh.cell(e).iter().map(|&p| rel(p)).collect();
            for (case, r) in rhs.iter_mut().enumerate() {
                for a in 0..nodes {
                    let f: f64 = (0..nodes).map(|b| ke[a * nodes + b] * x[b][case]).sum();
                    r[d[a] as usize] -= f;
                }
            }
        }
        if events.cancelled() {
            return Err("cancelled".into());
        }
        let points: Vec<[f64; 3]> = point_of_master(&mesh, &master, masters);
        let w = solve_cases(events, &k, &points, 1, &rhs, "conductivity")?;
        let mut kk = [[0.0; 3]; 3];
        for e in 0..mesh.len() {
            let ke = matrix(e);
            let d = &elements[e];
            let t: Vec<[f64; 3]> = (0..nodes)
                .map(|a| {
                    let x = rel(mesh.cell(e)[a]);
                    std::array::from_fn(|c| x[c] + w[c][d[a] as usize])
                })
                .collect();
            for i in 0..3 {
                for j in 0..3 {
                    let mut s = 0.0;
                    for a in 0..nodes {
                        for b in 0..nodes {
                            s += t[a][i] * ke[a * nodes + b] * t[b][j];
                        }
                    }
                    kk[i][j] += s / volume;
                }
            }
        }
        out.conductivity = Some(symmetric(kk));
        out.unknowns = out.unknowns.max(masters - 1);
    }
    if elastic {
        let mut cache: HashMap<([u64; 3], usize), Hex> = HashMap::new();
        let mut hex = |e: usize| -> Hex {
            let m = material_of(e);
            cache
                .entry((sizes[e].map(f64::to_bits), m))
                .or_insert_with(|| {
                    Hex::new(
                        sizes[e],
                        stiffness[m].as_ref().expect("a stiffness"),
                        model.materials[m].expansion,
                    )
                })
                .clone()
        };
        let dofs = |e: usize| -> [u32; 24] {
            let c = mesh.cell(e);
            std::array::from_fn(|r| 3 * master[c[r / 3] as usize] + (r % 3) as u32)
        };
        let n = 3 * masters;
        let elements: Vec<[u32; 24]> = (0..mesh.len()).map(dofs).collect();
        let mut k = Csr::pattern(n, elements.iter().map(|e| &e[..]));
        // 6 unit strains, then the unit temperature change.
        let mut rhs = vec![vec![0.0; n]; 7];
        for (e, d) in elements.iter().enumerate() {
            let h = hex(e);
            k.add_element(d, &h.k);
            let x: Vec<[f64; 3]> = mesh.cell(e).iter().map(|&p| rel(p)).collect();
            for case in 0..6 {
                let u = affine(case, &x);
                for r in 0..24 {
                    let f: f64 = (0..24).map(|c| h.k[r * 24 + c] * u[c]).sum();
                    rhs[case][d[r] as usize] -= f;
                }
            }
            for r in 0..24 {
                rhs[6][d[r] as usize] += (0..8).map(|c| h.load[r * 8 + c]).sum::<f64>();
            }
        }
        if events.cancelled() {
            return Err("cancelled".into());
        }
        let points = point_of_master(&mesh, &master, masters);
        let points: Vec<[f64; 3]> = points.iter().flat_map(|&p| [p; 3]).collect();
        let w = solve_cases(events, &k, &points, 3, &rhs, "stiffness")?;
        let mut c = [[0.0; 6]; 6];
        let mut sigma = [0.0; 6];
        for (e, d) in elements.iter().enumerate() {
            let h = hex(e);
            let x: Vec<[f64; 3]> = mesh.cell(e).iter().map(|&p| rel(p)).collect();
            let u: Vec<[f64; 24]> = (0..6)
                .map(|case| {
                    let a = affine(case, &x);
                    std::array::from_fn(|r| a[r] + w[case][d[r] as usize])
                })
                .collect();
            let ku: Vec<[f64; 24]> = u
                .iter()
                .map(|v| std::array::from_fn(|r| (0..24).map(|s| h.k[r * 24 + s] * v[s]).sum()))
                .collect();
            // K w - f of the thermal solve.
            let thermal: [f64; 24] = std::array::from_fn(|r| {
                (0..24)
                    .map(|s| h.k[r * 24 + s] * w[6][d[s] as usize])
                    .sum::<f64>()
                    - (0..8).map(|t| h.load[r * 8 + t]).sum::<f64>()
            });
            for i in 0..6 {
                for j in 0..6 {
                    c[i][j] += (0..24).map(|r| u[i][r] * ku[j][r]).sum::<f64>() / volume;
                }
                sigma[i] += (0..24).map(|r| u[i][r] * thermal[r]).sum::<f64>() / volume;
            }
        }
        let c = symmetric(c);
        let s = invert6(&c).ok_or(
            "the effective stiffness of the cell is singular: the solid does not connect \
             through the cell in each direction",
        )?;
        let alpha: [f64; 6] =
            std::array::from_fn(|i| -(0..6).map(|j| s[i][j] * sigma[j]).sum::<f64>());
        out.stiffness = Some(c);
        out.expansion = Some(alpha);
        out.unknowns = out.unknowns.max(n - 3);
    }
    out.mesh = mesh;
    Ok(out)
}

/// The mean part of the displacement of a unit strain `case` at the points `x`: x, y and z for
/// each point. The shear cases have the symmetric gradient, an engineering shear strain of 1.
fn affine(case: usize, x: &[[f64; 3]]) -> [f64; 24] {
    let mut g = [[0.0; 3]; 3];
    match case {
        0..=2 => g[case][case] = 1.0,
        _ => {
            let [a, b] = [[1, 2], [0, 2], [0, 1]][case - 3];
            g[a][b] = 0.5;
            g[b][a] = 0.5;
        }
    }
    std::array::from_fn(|r| (0..3).map(|j| g[r % 3][j] * x[r / 3][j]).sum())
}

/// The mean of a matrix and its transpose: the solves give a symmetric matrix to their
/// precision.
fn symmetric<const N: usize>(m: [[f64; N]; N]) -> [[f64; N]; N] {
    std::array::from_fn(|i| std::array::from_fn(|j| 0.5 * (m[i][j] + m[j][i])))
}

/// The inverse of a symmetric positive definite 6 x 6 matrix, or `None` when it is singular.
pub fn invert6(m: &[[f64; 6]; 6]) -> Option<[[f64; 6]; 6]> {
    let mut a = *m;
    let mut inv = [[0.0; 6]; 6];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    let scale = (0..6).map(|i| m[i][i].abs()).fold(0.0, f64::max);
    for col in 0..6 {
        let p = (col..6)
            .max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))
            .expect("a row");
        if !(a[p][col].abs() > 1e-12 * scale) {
            return None;
        }
        a.swap(col, p);
        inv.swap(col, p);
        let d = a[col][col];
        for j in 0..6 {
            a[col][j] /= d;
            inv[col][j] /= d;
        }
        for r in 0..6 {
            if r != col {
                let f = a[r][col];
                for j in 0..6 {
                    a[r][j] -= f * a[col][j];
                    inv[r][j] -= f * inv[col][j];
                }
            }
        }
    }
    Some(inv)
}

/// The coordinates of a point of each master.
fn point_of_master(mesh: &Mesh, master: &[u32], masters: usize) -> Vec<[f64; 3]> {
    let mut p = vec![[0.0; 3]; masters];
    for (i, &m) in master.iter().enumerate() {
        p[m as usize] = mesh.points[i];
    }
    p
}

/// The master of each point: the 2 points of each pair on opposite sides of the cell, and the
/// points that chain through pairs (the edges and the corners of the cell), share 1 master. The
/// masters are numbered from 0. An error tells that a point on a side has no partner.
fn pairs(mesh: &Mesh, lo: [f64; 3], hi: [f64; 3]) -> Result<Vec<u32>, String> {
    let n = mesh.points.len();
    let diag = (0..3).map(|d| (hi[d] - lo[d]).powi(2)).sum::<f64>().sqrt();
    let tol = 1e-9 * diag;
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    let mut root: Vec<usize> = (0..n).collect();
    for d in 0..3 {
        let (a, b) = ((d + 1) % 3, (d + 2) % 3);
        let key = |x: [f64; 3]| -> (i64, i64) {
            (
                ((x[a] - lo[a]) / tol).round() as i64,
                ((x[b] - lo[b]) / tol).round() as i64,
            )
        };
        let mut high: HashMap<(i64, i64), usize> = HashMap::new();
        let mut low = Vec::new();
        for (i, x) in mesh.points.iter().enumerate() {
            if (x[d] - hi[d]).abs() <= tol {
                high.insert(key(*x), i);
            } else if (x[d] - lo[d]).abs() <= tol {
                low.push(i);
            }
        }
        let axis = ["x", "y", "z"][d];
        let refuse = |x: [f64; 3], from: f64, to: f64| {
            format!(
                "the unit cell is not periodic: the point ({}, {}, {}) on the side {axis} = {from} \
                 has no partner on the side {axis} = {to}. The parts on 2 opposite sides of the \
                 cell must match, so that copies of the cell side by side fill the space",
                x[0], x[1], x[2]
            )
        };
        let mut matched = vec![false; n];
        for i in low {
            let (ka, kb) = key(mesh.points[i]);
            // A key 1 off: the 2 coordinates round on 2 sides of a step.
            let j = (-1..=1)
                .flat_map(|da| (-1..=1).map(move |db| (ka + da, kb + db)))
                .find_map(|k| high.get(&k).copied())
                .ok_or_else(|| refuse(mesh.points[i], lo[d], hi[d]))?;
            matched[j] = true;
            let (ri, rj) = (find(&mut root, i), find(&mut root, j));
            root[ri.max(rj)] = ri.min(rj);
        }
        if let Some(&j) = high.values().filter(|&&j| !matched[j]).min() {
            return Err(refuse(mesh.points[j], hi[d], lo[d]));
        }
    }
    let mut number = vec![u32::MAX; n];
    let mut master = vec![0u32; n];
    let mut count = 0u32;
    for i in 0..n {
        let r = find(&mut root, i);
        if number[r] == u32::MAX {
            number[r] = count;
            count += 1;
        }
        master[i] = number[r];
    }
    Ok(master)
}

/// A factor that needs more flops than this is refused. Reason: the factor of the stiffness of a
/// box cell of 19,652 points (52,269 unknowns) needed 1.4e11 flops and took 87 s (Apple M-series,
/// 1 thread), so 3e11 flops take about 3 minutes. The pairs couple the opposite sides, so the
/// factor of a cell costs about 3 times as much as that of the same grid with no pairs.
const MAX_FLOPS: f64 = 3e11;

/// Solve `a` x = b for each right side, with the unknowns of the first master (`per` of them)
/// fixed at 0. `what` names the property in an error.
fn solve_cases(
    events: &mut dyn Events,
    a: &Csr,
    points: &[[f64; 3]],
    per: usize,
    rhs: &[Vec<f64>],
    what: &str,
) -> Result<Vec<Vec<f64>>, String> {
    let keep: Vec<usize> = (per..a.n).collect();
    let sub = a.submatrix(&keep);
    let xyz: Vec<[f64; 3]> = keep.iter().map(|&i| points[i]).collect();
    let sym = Symbolic::analyse(&sub, &nested_dissection(&sub, &xyz));
    events.progress(&format!(
        "{what}: direct factor of {} unknowns, {:.1e} entries, {:.1e} flops",
        sub.n,
        sym.entries() as f64,
        sym.flops()
    ));
    if sym.flops() > MAX_FLOPS {
        return Err(format!(
            "the factor of the unit cell needs {:.1e} flops, more than the {MAX_FLOPS:.0e} of \
             about 3 minutes: use a larger max_cell or a larger max_growth",
            sym.flops()
        ));
    }
    if sym.entries() > DIRECT_ENTRIES {
        return Err(format!(
            "the factor of the unit cell has {:.1e} entries, more than the {DIRECT_ENTRIES:.1e} \
             that the memory of a laptop holds: use a larger max_cell or a larger max_growth",
            sym.entries() as f64
        ));
    }
    let chol = Cholesky::factor(&sym, &sub).map_err(|e| {
        format!(
            "the {what} problem of the unit cell has no single solution ({e}): a region of the \
             cell is free to move. Each region of solid must connect to the rest of the cell, \
             or to its copies through the opposite sides"
        )
    })?;
    Ok(rhs
        .iter()
        .map(|b| {
            let mut x: Vec<f64> = keep.iter().map(|&i| b[i]).collect();
            chol.solve(&mut x);
            let mut full = vec![0.0; a.n];
            for (k, &i) in keep.iter().enumerate() {
                full[i] = x[k];
            }
            full
        })
        .collect())
}

/// The effective material as engineering constants of an orthotropic material with its axes on
/// x, y and z: E (x, y, z), nu (xy, xz, yz) and G (xy, xz, yz), from the compliance C*^-1.
pub fn engineering(c: &[[f64; 6]; 6]) -> Option<Orthotropic> {
    let s = invert6(c)?;
    Some(Orthotropic {
        e: [1.0 / s[0][0], 1.0 / s[1][1], 1.0 / s[2][2]],
        nu: [-s[0][1] / s[0][0], -s[0][2] / s[0][0], -s[1][2] / s[1][1]],
        g: [1.0 / s[5][5], 1.0 / s[4][4], 1.0 / s[3][3]],
    })
}

/// A term that couples 2 axes and is below this fraction of the largest diagonal term is 0 to the
/// precision of the mesh. Reason: the periodic tetrahedral mesh of a square array of cylinders
/// gave K*_xy = 6e-7 of K*_zz, from the mesh that is not symmetric, where the exact value is 0. A
/// real coupling (a cell with no mirror plane) is of the order of the diagonal terms.
pub const COUPLING: f64 = 1e-4;

/// True when the terms of `m` that couple 2 axes or 2 shears are below `COUPLING` of its largest
/// diagonal term: then the material is orthotropic with its axes on x, y and z. `coupled(i, j)`
/// names the coupling terms.
fn axes_aligned<const N: usize>(m: &[[f64; N]; N], coupled: impl Fn(usize, usize) -> bool) -> bool {
    let scale = (0..N).map(|i| m[i][i].abs()).fold(0.0, f64::max);
    (0..N).all(|i| (0..N).all(|j| !coupled(i, j) || m[i][j].abs() <= COUPLING * scale))
}

/// True when the stiffness `c` is orthotropic with its axes on x, y and z: no normal-shear and no
/// shear-shear terms.
pub fn orthotropic(c: &[[f64; 6]; 6]) -> bool {
    axes_aligned(c, |i, j| i != j && (i >= 3 || j >= 3))
}

/// True when the conductivity `k` is diagonal.
pub fn diagonal(k: &[[f64; 3]; 3]) -> bool {
    axes_aligned(k, |i, j| i != j)
}
