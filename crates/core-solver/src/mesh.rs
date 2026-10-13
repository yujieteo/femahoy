//! The mesh of a run: node and element tables, dofs, materials, and the constraint
//! equations that the model implies.

use std::collections::{BTreeMap, HashMap};

use fh_inp::{CouplingKind, ElementTarget, Model, MpcKind, Target};

use crate::constraint::Elimination;
use crate::contribution::{Contribution, MAX_GROUP_NODES, MAX_GROUP_STATE};
use crate::element::{
    self, evaluate, ElementState, Evaluation, Kind, Material, Placement, MAX_DOFS, MAX_MAT_STATE,
    MAX_POINTS,
};
use crate::state::StateStore;
use crate::system::MaterialFactory;
use crate::SolverError;

pub(crate) struct Mesh<'a> {
    pub(crate) model: &'a Model,
    pub(crate) node_index: HashMap<u32, usize>,
    pub(crate) elem_index: HashMap<u32, usize>,
    /// First dof of each node.
    pub(crate) base: Vec<usize>,
    pub(crate) n_dofs: usize,
    pub(crate) kinds: Vec<Kind>,
    /// Index into `mats` for each element.
    pub(crate) mat: Vec<u16>,
    pub(crate) mats: Vec<Box<dyn Material>>,
    /// Node indices of all elements, then of all contribution groups, one after the other.
    /// An entry is an element (`e < kinds.len()`) or a group. Both add to the matrix the
    /// same way.
    pub(crate) conn: Vec<u32>,
    /// Position in `conn` of the first node of each entry, and one past the end.
    pub(crate) conn_start: Vec<u32>,
    /// Contributions that are not elements (contact in M6).
    pub(crate) contribs: Vec<Box<dyn Contribution>>,
    /// For each group entry: the contribution and the group number in it.
    pub(crate) groups: Vec<(u32, u32)>,
}

impl<'a> Mesh<'a> {
    pub(crate) fn new(
        model: &'a Model,
        factory: &MaterialFactory,
        contribs: Vec<Box<dyn Contribution>>,
    ) -> Result<Self, SolverError> {
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
        // Material of each element, from the sections. An element takes one section only.
        let mut material: HashMap<u32, &str> = HashMap::new();
        let mut section_of: HashMap<u32, usize> = HashMap::new();
        for (i, s) in model.sections.iter().enumerate() {
            let set = model
                .elset(&s.elset)
                .ok_or_else(|| SolverError::new(format!("element set {} not found", s.elset)))?;
            for id in &set.ids {
                match section_of.insert(*id, i) {
                    Some(j) if j != i => {
                        return Err(SolverError::new(format!(
                            "element {id} is in 2 *SOLID SECTION element sets ({} and {})",
                            model.sections[j].elset, s.elset
                        )));
                    }
                    _ => {
                        material.insert(*id, &s.material);
                    }
                }
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
                        .ok_or_else(|| SolverError::new(format!("material {name} not found")))?;
                    let built = factory.build(m)?;
                    if built.state_len() > MAX_MAT_STATE {
                        return Err(SolverError::new(format!(
                            "material {name} keeps {} state numbers, the limit is {MAX_MAT_STATE}",
                            built.state_len()
                        )));
                    }
                    let i = u16::try_from(mats.len())
                        .map_err(|_| SolverError::new("more than 65535 materials"))?;
                    mats.push(built);
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
        let mut groups = Vec::new();
        for (ci, c) in contribs.iter().enumerate() {
            if c.state_len() > MAX_GROUP_STATE {
                return Err(SolverError::new(format!(
                    "{} keeps {} state numbers for each group, the limit is {MAX_GROUP_STATE}",
                    c.name(),
                    c.state_len()
                )));
            }
            for (gi, nodes) in c.groups().iter().enumerate() {
                if nodes.is_empty() || nodes.len() > MAX_GROUP_NODES {
                    return Err(SolverError::new(format!(
                        "{}: a group has {} nodes, the limit is {MAX_GROUP_NODES}",
                        c.name(),
                        nodes.len()
                    )));
                }
                conn_start.push(conn.len() as u32);
                for n in nodes {
                    conn.push(node(*n)? as u32);
                }
                groups.push((ci as u32, gi as u32));
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
            contribs,
            groups,
        })
    }

    /// Elements and contribution groups.
    pub(crate) fn n_entries(&self) -> usize {
        self.conn_start.len() - 1
    }

    pub(crate) fn dof(&self, node: usize, d: u8) -> usize {
        self.base[node] + usize::from(d) - 1
    }

    pub(crate) fn node_dofs(&self, node: usize) -> usize {
        if node + 1 < self.base.len() {
            self.base[node + 1] - self.base[node]
        } else {
            self.n_dofs - self.base[node]
        }
    }

    pub(crate) fn check_dof(&self, node: usize, d: u8) -> Result<(), SolverError> {
        if usize::from(d) > self.node_dofs(node) {
            return Err(SolverError::new(format!(
                "node {} has no degree of freedom {d}",
                self.model.nodes[node].id
            )));
        }
        Ok(())
    }

    pub(crate) fn node(&self, id: u32) -> Result<usize, SolverError> {
        self.node_index
            .get(&id)
            .copied()
            .ok_or_else(|| SolverError::new(format!("node {id} not found")))
    }

    pub(crate) fn nodes_of(&self, t: &Target) -> Result<Vec<usize>, SolverError> {
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

    pub(crate) fn element(&self, id: u32) -> Result<usize, SolverError> {
        self.elem_index
            .get(&id)
            .copied()
            .ok_or_else(|| SolverError::new(format!("element {id} not found")))
    }

    pub(crate) fn elements_of(&self, t: &ElementTarget) -> Result<Vec<usize>, SolverError> {
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

    pub(crate) fn add_bcs(
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

    /// Mesh node indices of an element face, as `(element index, face)` gives them.
    pub(crate) fn face_node_indices(&self, e: usize, face: u8) -> Result<Vec<usize>, SolverError> {
        let local = element::face_nodes(self.kinds[e], face)?;
        let ids = &self.model.elements[e].nodes;
        local.iter().map(|&l| self.node(ids[l])).collect()
    }

    pub(crate) fn surface_nodes(
        &self,
        name: &str,
        ref_node: usize,
    ) -> Result<Vec<usize>, SolverError> {
        let mut v: Vec<usize> = if let Some(s) = self.model.surfaces.iter().find(|s| s.name == name)
        {
            s.ids
                .iter()
                .map(|id| self.node(*id))
                .collect::<Result<_, _>>()?
        } else if let Some(s) = self.model.element_surface(name) {
            let mut v = Vec::new();
            for &(id, face) in &s.faces {
                let e = *self
                    .elem_index
                    .get(&id)
                    .ok_or_else(|| SolverError::new(format!("element {id} not found")))?;
                v.extend(self.face_node_indices(e, face)?);
            }
            v
        } else {
            return Err(SolverError::new(format!("surface {name} not found")));
        };
        v.sort_unstable();
        v.dedup();
        v.retain(|&n| n != ref_node);
        Ok(v)
    }

    pub(crate) fn xyz(&self, node: usize) -> [f64; 3] {
        self.model.nodes[node].xyz
    }

    /// Prescribed values first, then the model constraints.
    pub(crate) fn elimination(
        &self,
        bc: &BTreeMap<(usize, u8), f64>,
    ) -> Result<Elimination, SolverError> {
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

    pub(crate) fn add_mpc(&self, el: &mut Elimination, m: &fh_inp::Mpc) -> Result<(), SolverError> {
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
                if r.iter().any(|x| !x.is_finite()) {
                    return Err(SolverError::new(
                        "*MPC LINK: the distance between the nodes is not finite",
                    ));
                }
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
    pub(crate) fn rot_terms(&self, node: usize, r: [f64; 3], c: usize) -> Vec<(usize, f64)> {
        // (theta x r)_c = theta_{c+1} r_{c+2} - theta_{c+2} r_{c+1}
        let (p, q) = ((c + 1) % 3, (c + 2) % 3);
        vec![
            (self.dof(node, p as u8 + 4), -r[q]),
            (self.dof(node, q as u8 + 4), r[p]),
        ]
    }

    pub(crate) fn add_coupling(
        &self,
        el: &mut Elimination,
        c: &fh_inp::Coupling,
    ) -> Result<(), SolverError> {
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
                let mut rows = 0;
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
                        rows += 1;
                    }
                    // A slave with rotation dofs turns with the reference.
                    if self.node_dofs(s) < 6 {
                        continue;
                    }
                    for d in (4..=6).filter(|d| in_range(*d)) {
                        el.add(
                            &[(self.dof(s, d), 1.0), (self.dof(rn, d), -1.0)],
                            0.0,
                            Some(self.dof(s, d)),
                        )?;
                        rows += 1;
                    }
                }
                if rows == 0 {
                    return Err(SolverError::new(format!(
                        "coupling {}: no slave node has a dof in the range {} to {}",
                        c.name, c.first_dof, c.last_dof
                    )));
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
    pub(crate) fn distributing(
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
    pub(crate) fn rotation_fit(
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
    pub(crate) fn element_dofs(&self, e: usize, dofs: &mut [usize; MAX_DOFS]) -> usize {
        let nodes = &self.conn[self.conn_start[e] as usize..self.conn_start[e + 1] as usize];
        for (a, &n) in nodes.iter().enumerate() {
            for c in 0..3 {
                dofs[3 * a + c] = self.base[n as usize] + c;
            }
        }
        3 * nodes.len()
    }

    pub(crate) fn element_coords(&self, e: usize, coords: &mut [[f64; 3]; 10]) -> usize {
        let nodes = &self.conn[self.conn_start[e] as usize..self.conn_start[e + 1] as usize];
        for (c, &n) in coords.iter_mut().zip(nodes) {
            *c = self.model.nodes[n as usize].xyz;
        }
        nodes.len()
    }

    pub(crate) fn element_error(&self, e: usize, err: SolverError) -> SolverError {
        SolverError::new(format!("element {}: {err}", self.model.elements[e].id))
    }

    /// The committed state of element `e` in the layout of the kernel.
    pub(crate) fn element_state(&self, states: &StateStore, e: usize) -> ElementState {
        let mut st = ElementState::default();
        let np = self.kinds[e].point_count();
        let slab = states.committed(e);
        for c in 0..self.mats[usize::from(self.mat[e])].state_len() {
            for i in 0..np {
                st.ip[i][c] = slab[c * np + i];
            }
        }
        st.internal = states.committed_internal(e);
        st
    }

    /// Write the trial state of an evaluation into the store.
    pub(crate) fn store_trial(&self, states: &mut StateStore, e: usize, ev: &Evaluation) {
        let np = self.kinds[e].point_count();
        let sl = self.mats[usize::from(self.mat[e])].state_len();
        if sl > 0 {
            let mut slab = [0.0; MAX_POINTS * MAX_MAT_STATE];
            for c in 0..sl {
                for i in 0..np {
                    slab[c * np + i] = ev.ip_state[i][c];
                }
            }
            states.set_trial(e, &slab[..sl * np]);
        }
        states.set_trial_internal(e, ev.internal);
    }

    /// Numbers of material state of each element.
    pub(crate) fn state_lens(&self) -> Vec<usize> {
        (0..self.kinds.len())
            .map(|e| self.kinds[e].point_count() * self.mats[usize::from(self.mat[e])].state_len())
            .collect()
    }

    pub(crate) fn all_linear(&self) -> bool {
        self.mats.iter().all(|m| m.is_linear())
    }

    /// True when the run takes the linear fallback: every material is linear and there is
    /// no contribution (no contact).
    pub(crate) fn linear(&self) -> bool {
        self.all_linear() && self.contribs.is_empty()
    }

    /// Numbers of state of each contribution group.
    pub(crate) fn group_state_lens(&self) -> Vec<usize> {
        self.groups
            .iter()
            .map(|&(c, _)| self.contribs[c as usize].state_len())
            .collect()
    }

    /// Evaluate group `g` (counted from 0 among the groups) at the displacements `u` of its
    /// nodes. `old` is its committed state.
    pub(crate) fn evaluate_group(
        &self,
        g: usize,
        u: &[f64],
        old: &[f64],
        out: &mut Evaluation,
        tangent: bool,
    ) -> Result<(), SolverError> {
        let mut coords = [[0.0; 3]; 10];
        let n = self.element_coords(self.kinds.len() + g, &mut coords);
        let (c, gi) = self.groups[g];
        let contrib = &self.contribs[c as usize];
        let place = Placement {
            start: &coords[..n],
            disp: &u[..3 * n],
        };
        // The contribution adds into a clean output. Its trial state starts as the committed one.
        out.n = 3 * n;
        out.fint = [0.0; MAX_DOFS];
        out.fabs = [0.0; MAX_DOFS];
        out.k = [[0.0; MAX_DOFS]; MAX_DOFS];
        out.state_mut()[..old.len()].copy_from_slice(old);
        contrib
            .evaluate(gi as usize, &place, old, out, tangent)
            .map_err(|e| SolverError::new(format!("{}: {e}", contrib.name())))
    }

    /// Evaluate element `e` at the displacements `u` (its `3n` numbers).
    pub(crate) fn evaluate_element(
        &self,
        e: usize,
        u: &[f64],
        state: &ElementState,
        out: &mut Evaluation,
        tangent: bool,
    ) -> Result<(), SolverError> {
        let mut coords = [[0.0; 3]; 10];
        let n = self.element_coords(e, &mut coords);
        let material: &dyn Material = &*self.mats[usize::from(self.mat[e])];
        let place = Placement {
            start: &coords[..n],
            disp: &u[..3 * n],
        };
        evaluate(self.kinds[e], &place, material, state, out, tangent)
            .map_err(|err| self.element_error(e, err))
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The length of `a`, scaled by its largest component so that the squares do not
/// overflow or underflow.
fn norm(a: [f64; 3]) -> f64 {
    let m = a.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    if m == 0.0 || !m.is_finite() {
        return m;
    }
    let s = a.map(|x| x / m);
    m * dot(s, s).sqrt()
}
