//! Build the linear system from a model, eliminate constraints and solve.
//!
//! Steps are linear. Each step solves once. Boundary conditions, concentrated
//! loads and pressure loads carry over to later steps; a later value for the
//! same dof or face replaces the earlier one.

use std::collections::{BTreeMap, HashMap};

use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMat, Triplet};
use faer::{Mat, Side};
use fh_inp::{CouplingKind, ElementTarget, Model, MpcKind, Target};

use crate::constraint::{Elimination, Transform};
use crate::element::{self, elasticity, Kind};
use crate::SolverError;

/// Results of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepResult {
    pub name: Option<String>,
    /// Translations, in the order of `Model::nodes`.
    pub displacement: Vec<[f64; 3]>,
    /// Rotations. Zero for nodes without rotational dofs.
    pub rotation: Vec<[f64; 3]>,
    /// Internal force minus applied load, per node. Non-zero at supports.
    pub reaction: Vec<[f64; 3]>,
    /// Volume-averaged stress `[xx, yy, zz, xy, yz, xz]`, in the order of `Model::elements`.
    pub element_stress: Vec<[f64; 6]>,
    pub mises: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub steps: Vec<StepResult>,
    pub total_dofs: usize,
    /// Dofs left after elimination, per step.
    pub free_dofs: Vec<usize>,
}

struct Mesh<'a> {
    model: &'a Model,
    node_index: HashMap<u32, usize>,
    /// First dof of each node.
    base: Vec<usize>,
    n_dofs: usize,
    kinds: Vec<Kind>,
    /// Elasticity matrix per element.
    d: Vec<[[f64; 6]; 6]>,
    /// Node indices per element.
    conn: Vec<Vec<usize>>,
}

/// Solve every step of the model.
pub fn solve(model: &Model) -> Result<Solution, SolverError> {
    let mesh = Mesh::new(model)?;
    let mut bc: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut loads: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    let mut pressures: BTreeMap<(usize, u8), f64> = BTreeMap::new();
    mesh.add_bcs(&model.boundaries, &mut bc)?;
    let mut steps = Vec::new();
    let mut free_dofs = Vec::new();
    if model.steps.is_empty() {
        return Err(SolverError::new("the model has no *STEP"));
    }
    for step in &model.steps {
        mesh.add_bcs(&step.boundaries, &mut bc)?;
        for c in &step.cloads {
            for n in mesh.nodes_of(&c.target)? {
                mesh.check_dof(n, c.dof)?;
                loads.insert((n, c.dof), c.magnitude);
            }
        }
        for dl in &step.dloads {
            for e in mesh.elements_of(&dl.target)? {
                pressures.insert((e, dl.face), dl.pressure);
            }
        }
        let t = mesh.transform(&bc)?;
        free_dofs.push(t.n_free);
        steps.push(mesh.solve_step(step.name.clone(), &t, &loads, &pressures)?);
    }
    Ok(Solution {
        steps,
        total_dofs: mesh.n_dofs,
        free_dofs,
    })
}

impl<'a> Mesh<'a> {
    fn new(model: &'a Model) -> Result<Self, SolverError> {
        let node_index: HashMap<u32, usize> = model
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();
        // Nodes that carry rotations: reference nodes and rigid beam ends.
        let mut rot = vec![false; model.nodes.len()];
        for c in &model.couplings {
            if c.kind == CouplingKind::Kinematic || c.last_dof >= 4 {
                rot[node_index[&c.ref_node]] = true;
            }
        }
        for m in model.mpcs.iter().filter(|m| m.kind == MpcKind::Beam) {
            for n in &m.nodes {
                rot[node_index[n]] = true;
            }
        }
        let mut base = Vec::with_capacity(rot.len());
        let mut n_dofs = 0;
        for r in &rot {
            base.push(n_dofs);
            n_dofs += if *r { 6 } else { 3 };
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
        let (mut kinds, mut d, mut conn) = (Vec::new(), Vec::new(), Vec::new());
        for el in &model.elements {
            let kind = Kind::parse(&el.element_type).ok_or_else(|| {
                SolverError::new(format!(
                    "element {}: type {} is not available yet",
                    el.id, el.element_type
                ))
            })?;
            let mat = material.get(&el.id).ok_or_else(|| {
                SolverError::new(format!("element {} has no *SOLID SECTION", el.id))
            })?;
            let el_mat = model
                .material(mat)
                .and_then(|m| m.elastic.as_ref())
                .ok_or_else(|| SolverError::new(format!("material {mat} has no *ELASTIC")))?;
            kinds.push(kind);
            d.push(elasticity(el_mat.youngs_modulus, el_mat.poissons_ratio));
            conn.push(el.nodes.iter().map(|n| node_index[n]).collect());
        }
        Ok(Mesh {
            model,
            node_index,
            base,
            n_dofs,
            kinds,
            d,
            conn,
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

    fn nodes_of(&self, t: &Target) -> Result<Vec<usize>, SolverError> {
        Ok(match t {
            Target::Node(id) => vec![self.node_index[id]],
            Target::Set(name) => {
                let set = self
                    .model
                    .nset(name)
                    .ok_or_else(|| SolverError::new(format!("node set {name} not found")))?;
                set.ids.iter().map(|id| self.node_index[id]).collect()
            }
        })
    }

    fn elements_of(&self, t: &ElementTarget) -> Result<Vec<usize>, SolverError> {
        let pos: HashMap<u32, usize> = self
            .model
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        Ok(match t {
            ElementTarget::Element(id) => vec![pos[id]],
            ElementTarget::Set(name) => {
                let set = self
                    .model
                    .elset(name)
                    .ok_or_else(|| SolverError::new(format!("element set {name} not found")))?;
                set.ids.iter().map(|id| pos[id]).collect()
            }
        })
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
        let mut v: Vec<usize> = s.ids.iter().map(|id| self.node_index[id]).collect();
        v.sort_unstable();
        v.dedup();
        v.retain(|&n| n != ref_node);
        Ok(v)
    }

    fn xyz(&self, node: usize) -> [f64; 3] {
        self.model.nodes[node].xyz
    }

    /// Prescribed values first, then the model constraints.
    fn transform(&self, bc: &BTreeMap<(usize, u8), f64>) -> Result<Transform, SolverError> {
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
                    let n = self.node_index[&t.node];
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
        Ok(el.finish())
    }

    fn add_mpc(&self, el: &mut Elimination, m: &fh_inp::Mpc) -> Result<(), SolverError> {
        let (a, b) = (self.node_index[&m.nodes[0]], self.node_index[&m.nodes[1]]);
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
        let rn = self.node_index[&c.ref_node];
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
            CouplingKind::Distributing => {
                let w = 1.0 / slaves.len() as f64;
                for k in 0..3 {
                    if in_range(k as u8 + 1) {
                        let mut row = vec![(self.dof(rn, k as u8 + 1), 1.0)];
                        row.extend(slaves.iter().map(|&s| (self.dof(s, k as u8 + 1), -w)));
                        el.add(&row, 0.0, Some(self.dof(rn, k as u8 + 1)))?;
                    }
                }
                if c.last_dof >= 4 {
                    self.distributing_rotation(el, c, rn, &slaves, w)?;
                }
            }
        }
        Ok(())
    }

    /// Reference rotation from a least-squares rigid fit of the slave motion.
    fn distributing_rotation(
        &self,
        el: &mut Elimination,
        c: &fh_inp::Coupling,
        rn: usize,
        slaves: &[usize],
        w: f64,
    ) -> Result<(), SolverError> {
        let mut centre = [0.0; 3];
        for &s in slaves {
            for (c, x) in centre.iter_mut().zip(self.xyz(s)) {
                *c += w * x;
            }
        }
        let mut a = [[0.0; 3]; 3];
        for &s in slaves {
            let r = sub(self.xyz(s), centre);
            for i in 0..3 {
                for j in 0..3 {
                    a[i][j] += w * (if i == j { dot(r, r) } else { 0.0 } - r[i] * r[j]);
                }
            }
        }
        let flat: Vec<f64> = a.iter().flatten().copied().collect();
        let scale = flat.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let inv = crate::dense::invert(&flat, 3)
            .filter(|_| {
                let (_, det) = crate::dense::inv3(&a);
                det.abs() > 1e-12 * scale.powi(3)
            })
            .ok_or_else(|| {
                SolverError::new(format!(
                    "coupling {}: slave nodes are on a line, rotation is undefined",
                    c.name
                ))
            })?;
        for k in 0..3u8 {
            if !(c.first_dof..=c.last_dof).contains(&(k + 4)) {
                continue;
            }
            // theta = A^-1 * sum(w r x u); (r x u)_m = eps_mnp r_n u_p.
            let mut terms: BTreeMap<usize, f64> = BTreeMap::new();
            for &s in slaves {
                let r = sub(self.xyz(s), centre);
                let rx = [[0.0, -r[2], r[1]], [r[2], 0.0, -r[0]], [-r[1], r[0], 0.0]];
                for p in 0..3 {
                    let coef: f64 = rx
                        .iter()
                        .enumerate()
                        .map(|(m, row)| inv[usize::from(k) * 3 + m] * row[p])
                        .sum::<f64>()
                        * w;
                    *terms.entry(self.dof(s, p as u8 + 1)).or_insert(0.0) += coef;
                }
            }
            let mut row = vec![(self.dof(rn, k + 4), 1.0)];
            row.extend(terms.into_iter().map(|(d, v)| (d, -v)));
            el.add(&row, 0.0, Some(self.dof(rn, k + 4)))?;
        }
        Ok(())
    }

    fn element_coords(&self, e: usize) -> Vec<[f64; 3]> {
        self.conn[e].iter().map(|&n| self.xyz(n)).collect()
    }

    fn element_dofs(&self, e: usize) -> Vec<usize> {
        self.conn[e]
            .iter()
            .flat_map(|&n| (1..=3).map(move |d| (n, d)))
            .map(|(n, d)| self.dof(n, d))
            .collect()
    }

    fn solve_step(
        &self,
        name: Option<String>,
        t: &Transform,
        loads: &BTreeMap<(usize, u8), f64>,
        pressures: &BTreeMap<(usize, u8), f64>,
    ) -> Result<StepResult, SolverError> {
        let mut f = vec![0.0; self.n_dofs];
        for (&(n, d), &v) in loads {
            f[self.dof(n, d)] += v;
        }
        for (&(e, face), &p) in pressures {
            let coords = self.element_coords(e);
            for (local, force) in
                element::pressure_load(self.kinds[e], &coords, face, p).map_err(|err| {
                    SolverError::new(format!("element {}: {err}", self.model.elements[e].id))
                })?
            {
                for (k, v) in force.iter().enumerate() {
                    f[self.dof(self.conn[e][local], k as u8 + 1)] += v;
                }
            }
        }
        // Assemble Tᵀ K T and the right side Tᵀ (f - K u0).
        let mut triplets = Vec::new();
        let mut rhs_full = f.clone();
        for e in 0..self.conn.len() {
            let dofs = self.element_dofs(e);
            let ke = element::stiffness(self.kinds[e], &self.element_coords(e), &self.d[e])
                .map_err(|err| {
                    SolverError::new(format!("element {}: {err}", self.model.elements[e].id))
                })?;
            let n = dofs.len();
            let u0: Vec<f64> = dofs.iter().map(|&d| t.offset[d]).collect();
            for i in 0..n {
                if u0.iter().any(|v| *v != 0.0) {
                    rhs_full[dofs[i]] -= (0..n).map(|j| ke[i * n + j] * u0[j]).sum::<f64>();
                }
                for j in 0..n {
                    let v = ke[i * n + j];
                    for &(a, ca) in &t.rows[dofs[i]] {
                        for &(b, cb) in &t.rows[dofs[j]] {
                            triplets.push(Triplet::new(a, b, ca * cb * v));
                        }
                    }
                }
            }
        }
        let mut ur = vec![0.0; t.n_free];
        if t.n_free > 0 {
            let fr = t.reduce(&rhs_full);
            let k =
                SparseColMat::<usize, f64>::try_new_from_triplets(t.n_free, t.n_free, &triplets)
                    .map_err(|e| SolverError::new(format!("matrix assembly failed: {e:?}")))?;
            let llt = k.sp_cholesky(Side::Lower).map_err(|_| {
                SolverError::new(
                    "stiffness matrix is not positive definite: check the boundary conditions for rigid body motion",
                )
            })?;
            let mut x = Mat::<f64>::from_fn(t.n_free, 1, |i, _| fr[i]);
            llt.solve_in_place(x.as_mut());
            for (i, v) in ur.iter_mut().enumerate() {
                *v = x[(i, 0)];
            }
        }
        let u = t.expand(&ur);
        // Reactions and stress from the element forces.
        let mut fint = vec![0.0; self.n_dofs];
        let mut stress = Vec::with_capacity(self.conn.len());
        for e in 0..self.conn.len() {
            let dofs = self.element_dofs(e);
            let ue: Vec<f64> = dofs.iter().map(|&d| u[d]).collect();
            let (fe, s) =
                element::response(self.kinds[e], &self.element_coords(e), &self.d[e], &ue)
                    .map_err(|err| {
                        SolverError::new(format!("element {}: {err}", self.model.elements[e].id))
                    })?;
            for (d, v) in dofs.iter().zip(fe) {
                fint[*d] += v;
            }
            stress.push(s);
        }
        let n_nodes = self.model.nodes.len();
        let grab = |v: &[f64], first: u8| -> Vec<[f64; 3]> {
            (0..n_nodes)
                .map(|n| {
                    let mut o = [0.0; 3];
                    for (k, x) in o.iter_mut().enumerate() {
                        if usize::from(first) + k <= self.node_dofs(n) {
                            *x = v[self.dof(n, first + k as u8)];
                        }
                    }
                    o
                })
                .collect()
        };
        let resid: Vec<f64> = fint.iter().zip(&f).map(|(a, b)| a - b).collect();
        Ok(StepResult {
            name,
            displacement: grab(&u, 1),
            rotation: grab(&u, 4),
            reaction: grab(&resid, 1),
            mises: stress.iter().map(element::von_mises).collect(),
            element_stress: stress,
        })
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
