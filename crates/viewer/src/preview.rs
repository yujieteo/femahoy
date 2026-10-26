//! The supports and loads of a solver input: the surface faces of each
//! support and each load, the direction and the total of each load, and a label for each set.
//! The page shows them before the solve and on the results. The input is the copy of the
//! input file that the run solves, so the page shows what the solver gets.
//!
//! Plain functions with no GPU types, so unit tests run without a window.

use std::collections::{BTreeMap, HashMap, HashSet};

use fh_core::inp::{ElementTarget, Model, Target};

use crate::mesh::{self, RenderMesh};
use crate::results::{unit, Quantity, BANDS};
use crate::view::format_value;

/// The face colours. Slot 0 (grey) is a face with no support and no load. Slots 1 to 4 are
/// for the supports, 5 to 8 for the loads.
pub const COLORS: [[f32; 3]; BANDS] = [
    [0.62, 0.62, 0.62],
    [0.10, 0.40, 0.95],
    [0.00, 0.70, 0.75],
    [0.50, 0.30, 0.90],
    [0.15, 0.65, 0.25],
    [0.95, 0.15, 0.10],
    [1.00, 0.55, 0.00],
    [0.90, 0.15, 0.70],
    [0.85, 0.75, 0.00],
];

/// The length of a load arrow, as a part of the half diagonal of the model.
const ARROW: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Support,
    Load,
}

/// The supports or the loads on one set of the input.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The name of the node set or the surface in the input.
    pub name: String,
    pub kind: Kind,
    /// One text for each data line of the input, with its values as they are in the input,
    /// for example `U1 to U3 = 0`.
    pub actions: Vec<String>,
    /// The text on the model: the name and what acts on the set, for example
    /// `FIXED: ux uy uz` or `P1: 2700 N, -y`.
    pub label: String,
    /// The slot of the colour in `COLORS`.
    pub color: usize,
    /// The surface triangles of the set: indices into `mesh.triangles`.
    pub triangles: Vec<u32>,
    /// The outline of the triangles: each edge (2 positions of the mesh) with the outward
    /// normal of its triangle.
    pub outline: Vec<([u32; 2], [f32; 3])>,
    pub nodes: usize,
    /// The area of the triangles.
    pub area: f64,
    /// The mean point of the triangles by area, or of the nodes when there is no triangle.
    pub center: [f64; 3],
    /// The sum of the outward area vectors of the triangles.
    pub outward: [f64; 3],
    /// The total force of the loads. Zero for a support.
    pub force: [f64; 3],
}

/// A solver input with its supports and loads.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// The name of the model in the page, for example the input file.
    pub title: String,
    pub units: String,
    pub mesh: RenderMesh,
    pub items: Vec<Item>,
}

/// A set of the input during the build: its nodes, or the element faces of a surface.
struct Draft {
    item: Item,
    surface: bool,
    nodes: HashSet<u32>,
    /// The node sets of the faces of each element of a surface, by element index.
    faces: HashMap<u32, Vec<HashSet<u32>>>,
    pressure: f64,
    /// The force on each node of a load, for each degree of freedom.
    cload: BTreeMap<u8, f64>,
    /// The value of each fixed degree of freedom of a support.
    fixed: BTreeMap<u8, f64>,
}

fn dofs(first: u8, last: u8) -> String {
    let name = |d: u8| match d {
        1..=3 => format!("U{d}"),
        11 => "Temperature".to_owned(),
        _ => format!("DOF {d}"),
    };
    if last > first {
        format!("{} to {}", name(first), name(last))
    } else {
        name(first)
    }
}

/// A value and its unit, or the value alone when there is no unit.
fn with_unit(value: String, unit: &str) -> String {
    if unit.is_empty() {
        value
    } else {
        format!("{value} {unit}")
    }
}

/// The fixed degrees of freedom of a support, for example `ux uy uz` or `uy, uz = 0.1 mm`.
fn fixed_text(fixed: &BTreeMap<u8, f64>, length_unit: &str) -> String {
    let name = |d: u8| match d {
        1..=3 => (
            ["ux", "uy", "uz"][usize::from(d) - 1].to_owned(),
            length_unit,
        ),
        11 => ("temperature".to_owned(), ""),
        _ => (format!("dof {d}"), ""),
    };
    let zero = fixed.iter().filter(|(_, &v)| v == 0.0);
    let zero: Vec<String> = zero.map(|(&d, _)| name(d).0).collect();
    let set = fixed.iter().filter(|(_, &v)| v != 0.0).map(|(&d, &v)| {
        let (name, unit) = name(d);
        format!("{name} = {}", with_unit(v.to_string(), unit))
    });
    let words = Some(zero.join(" ")).filter(|z| !z.is_empty());
    words.into_iter().chain(set).collect::<Vec<_>>().join(", ")
}

/// A value with 4 significant digits and no zeros at the end, for a label.
fn short(v: f64) -> String {
    let s = format_value(v);
    match s.contains('.') && !s.contains(['e', 'E']) {
        true => s.trim_end_matches('0').trim_end_matches('.').to_owned(),
        false => s,
    }
}

/// The direction of a force: an axis, for example `-y`, or else the unit vector.
fn direction(f: [f64; 3]) -> String {
    let size = length(f);
    let axes: Vec<usize> = (0..3).filter(|&k| f[k].abs() > 1e-9 * size).collect();
    match axes[..] {
        [k] => format!(
            "{}{}",
            if f[k] < 0.0 { "-" } else { "+" },
            ["x", "y", "z"][k]
        ),
        _ => {
            let u = f.map(|x| short(x / size));
            format!("({}, {}, {})", u[0], u[1], u[2])
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Read the supports (`*BOUNDARY`) and the loads (`*CLOAD`, `*DLOAD`, `*DSLOAD`) of a model, and
/// the surface triangles of each set.
pub fn build(model: &Model, units: &str) -> Preview {
    let mesh = mesh::build(model);
    let index: HashMap<u32, u32> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    let element: HashMap<u32, usize> = model
        .elements
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id, i))
        .collect();
    let element_nodes = |e: usize| -> Vec<u32> {
        let ids = &model.elements[e].nodes;
        ids.iter().filter_map(|id| index.get(id).copied()).collect()
    };
    let mut drafts: Vec<Draft> = Vec::new();
    // The draft of a set, new or found.
    let draft = |drafts: &mut Vec<Draft>, name: &str, kind: Kind, surface: bool| {
        let same = |d: &Draft| {
            d.item.kind == kind && d.surface == surface && d.item.name.eq_ignore_ascii_case(name)
        };
        if let Some(i) = drafts.iter().position(same) {
            return i;
        }
        let d = Draft {
            item: Item {
                name: name.to_owned(),
                kind,
                actions: Vec::new(),
                label: String::new(),
                color: 0,
                triangles: Vec::new(),
                outline: Vec::new(),
                nodes: 0,
                area: 0.0,
                center: [0.0; 3],
                outward: [0.0; 3],
                force: [0.0; 3],
            },
            surface,
            nodes: HashSet::new(),
            faces: HashMap::new(),
            pressure: 0.0,
            cload: BTreeMap::new(),
            fixed: BTreeMap::new(),
        };
        drafts.push(d);
        drafts.len() - 1
    };
    // Add the faces `(element id, face number)` to a surface draft, with the nodes of each.
    let add_faces = |d: &mut Draft, faces: &[(u32, u8)]| {
        for &(id, face) in faces {
            let Some(&e) = element.get(&id) else {
                continue;
            };
            let element = &model.elements[e];
            let Some(f) = mesh::surface_face(&element.element_type, &element_nodes(e), face) else {
                continue;
            };
            let nodes: HashSet<u32> = f
                .corners
                .iter()
                .chain(f.mids.iter().flatten())
                .copied()
                .collect();
            d.nodes.extend(&nodes);
            d.faces.entry(e as u32).or_default().push(nodes);
        }
    };
    let nodes_of = |target: &Target| -> (String, HashSet<u32>) {
        match target {
            Target::Node(id) => (
                format!("node {id}"),
                index.get(id).copied().into_iter().collect(),
            ),
            Target::Set(name) => {
                let ids = model.nset(name).map_or(&[][..], |s| &s.ids);
                (
                    name.clone(),
                    ids.iter().filter_map(|id| index.get(id).copied()).collect(),
                )
            }
        }
    };
    let (length_unit, stress, force) = (
        unit(units, Quantity::Displacement),
        unit(units, Quantity::Stress),
        unit(units, Quantity::Force),
    );
    // A later line for the same set (and degree of freedom) replaces the value of an earlier
    // line, as a load of a later step replaces the load of an earlier step. With more than 1
    // step, each action names its step.
    let tag = |k: usize| match model.steps.len() {
        0 | 1 => String::new(),
        _ => match &model.steps[k].name {
            Some(name) => format!("{name}: "),
            None => format!("step {}: ", k + 1),
        },
    };
    let steps = || model.steps.iter().enumerate();
    let boundaries = model.boundaries.iter().map(|b| (String::new(), b));
    let in_steps = steps().flat_map(|(k, s)| s.boundaries.iter().map(move |b| (tag(k), b)));
    for (tag, b) in boundaries.chain(in_steps) {
        let (name, nodes) = nodes_of(&b.target);
        let i = draft(&mut drafts, &name, Kind::Support, false);
        // Only U1 to U3 are lengths. DOF 4 to 6 are rotations, DOF 11 is the temperature.
        let value = match (b.value == 0.0, b.last_dof.max(b.first_dof) <= 3) {
            (true, _) => "0".to_owned(),
            (false, true) => with_unit(b.value.to_string(), length_unit),
            (false, false) => b.value.to_string(),
        };
        let dofs = dofs(b.first_dof, b.last_dof);
        drafts[i]
            .item
            .actions
            .push(format!("{tag}{dofs} = {value}"));
        for d in b.first_dof..=b.last_dof.max(b.first_dof) {
            drafts[i].fixed.insert(d, b.value);
        }
        drafts[i].nodes = nodes;
    }
    for (tag, c) in steps().flat_map(|(k, s)| s.cloads.iter().map(move |c| (tag(k), c))) {
        let (name, nodes) = nodes_of(&c.target);
        let i = draft(&mut drafts, &name, Kind::Load, false);
        let n = nodes.len();
        let on = if n == 1 {
            "1 node".to_owned()
        } else {
            format!("each of {n} nodes")
        };
        let (dof, m) = (c.dof, c.magnitude);
        drafts[i]
            .item
            .actions
            .push(format!("{tag}F{dof} = {m} {force} on {on}"));
        drafts[i].cload.insert(dof, m);
        drafts[i].nodes = nodes;
    }
    for (tag, d) in steps().flat_map(|(k, s)| s.dsloads.iter().map(move |d| (tag(k), d))) {
        let i = draft(&mut drafts, &d.surface, Kind::Load, true);
        let faces = model
            .element_surface(&d.surface)
            .map_or(&[][..], |s| &s.faces);
        add_faces(&mut drafts[i], faces);
        let p = d.pressure;
        drafts[i].pressure = p;
        drafts[i]
            .item
            .actions
            .push(format!("{tag}pressure {p} {stress}"));
    }
    for (tag, l) in steps().flat_map(|(k, s)| s.dloads.iter().map(move |l| (tag(k), l))) {
        let (name, ids) = match &l.target {
            ElementTarget::Element(id) => (format!("element {id}"), vec![*id]),
            ElementTarget::Set(name) => {
                let ids = model.elset(name).map_or_else(Vec::new, |s| s.ids.clone());
                (name.clone(), ids)
            }
        };
        let i = draft(&mut drafts, &name, Kind::Load, true);
        // A shell load has no face number: it acts on the shell face.
        let faces: Vec<(u32, u8)> = ids.iter().map(|&id| (id, l.face.max(1))).collect();
        add_faces(&mut drafts[i], &faces);
        let p = l.pressure;
        drafts[i].pressure = p;
        drafts[i]
            .item
            .actions
            .push(format!("{tag}pressure {p} {stress}"));
    }

    let points: Vec<[f64; 3]> = model.nodes.iter().map(|n| n.xyz).collect();
    let centroids: Vec<[f64; 3]> = (0..model.elements.len())
        .map(|e| {
            let nodes = element_nodes(e);
            let mut c = [0.0; 3];
            for &n in &nodes {
                (0..3).for_each(|k| c[k] += points[n as usize][k] / nodes.len() as f64);
            }
            c
        })
        .collect();
    let (mut supports, mut loads) = (0, 0);
    let items = drafts
        .into_iter()
        .map(|mut d| {
            let mut weighted = [0.0; 3];
            // Each edge of the triangles: the number of triangles and the normal of the last.
            let mut edges: BTreeMap<[u32; 2], (usize, [f32; 3])> = BTreeMap::new();
            for (t, tri) in mesh.triangles.iter().enumerate() {
                let nodes = tri.map(|p| mesh.source_node[p as usize]);
                let cell = mesh.triangle_cell[t];
                let inside = match d.surface {
                    true => d.faces.get(&cell).is_some_and(|faces| {
                        faces.iter().any(|f| nodes.iter().all(|n| f.contains(n)))
                    }),
                    false => nodes.iter().all(|n| d.nodes.contains(n)),
                };
                if !inside {
                    continue;
                }
                let [a, b, c] = nodes.map(|n| points[n as usize]);
                let (u, v) = (sub(b, a), sub(c, a));
                let mut area = [
                    0.5 * (u[1] * v[2] - u[2] * v[1]),
                    0.5 * (u[2] * v[0] - u[0] * v[2]),
                    0.5 * (u[0] * v[1] - u[1] * v[0]),
                ];
                // A point in the plane of the triangle (a shell) does not flip the vector.
                let inward = sub(centroids[cell as usize], a);
                if dot(area, inward) > 1e-9 * length(area) * length(inward) {
                    area = area.map(|x| -x);
                }
                let normal = area.map(|x| (x / length(area)) as f32);
                for k in 0..3 {
                    let (a, b) = (tri[k], tri[(k + 1) % 3]);
                    let edge = edges.entry([a.min(b), a.max(b)]).or_default();
                    *edge = (edge.0 + 1, normal);
                }
                let it = &mut d.item;
                it.triangles.push(t as u32);
                it.area += length(area);
                (0..3).for_each(|k| {
                    it.outward[k] += area[k];
                    weighted[k] += length(area) * (a[k] + b[k] + c[k]) / 3.0;
                });
            }
            let it = &mut d.item;
            it.outline = edges
                .into_iter()
                .filter(|(_, (n, _))| *n == 1)
                .map(|(e, (_, normal))| (e, normal))
                .collect();
            for (&dof, &m) in d.cload.iter().filter(|(&dof, _)| (1..=3).contains(&dof)) {
                it.force[usize::from(dof) - 1] += m * d.nodes.len() as f64;
            }
            // Positive pressure pushes into the body, against the outward normal.
            (0..3).for_each(|k| it.force[k] -= d.pressure * it.outward[k]);
            it.nodes = d.nodes.len();
            it.center = if it.area > 0.0 {
                weighted.map(|w| w / it.area)
            } else if !d.nodes.is_empty() {
                let mut c = [0.0; 3];
                for &n in &d.nodes {
                    (0..3).for_each(|k| c[k] += points[n as usize][k] / d.nodes.len() as f64);
                }
                c
            } else {
                [0.0; 3]
            };
            let size = length(it.force);
            let text = match it.kind {
                Kind::Support => fixed_text(&d.fixed, length_unit),
                Kind::Load if d.surface => {
                    format!("{} pressure", with_unit(d.pressure.to_string(), stress))
                }
                Kind::Load if size > 0.0 => {
                    format!("{}, {}", with_unit(short(size), force), direction(it.force))
                }
                Kind::Load => with_unit("0".into(), force),
            };
            it.label = format!("{}: {text}", it.name);
            let count = match it.kind {
                Kind::Support => &mut supports,
                _ => &mut loads,
            };
            it.color = 1 + 4 * usize::from(it.kind != Kind::Support) + *count % 4;
            *count += 1;
            d.item
        })
        .collect();
    Preview {
        title: String::new(),
        units: units.to_owned(),
        mesh,
        items,
    }
}

impl Preview {
    /// The contour position of each triangle corner, for the bands of `COLORS`.
    pub fn face_values(&self) -> Vec<f32> {
        let at = |slot: u8| (f32::from(slot) + 0.5) / BANDS as f32;
        self.slots().into_iter().flat_map(|s| [at(s); 3]).collect()
    }

    /// The colour slot of each triangle. A load colour covers a support colour.
    pub fn slots(&self) -> Vec<u8> {
        let mut slots = vec![0u8; self.mesh.triangles.len()];
        for kind in [Kind::Support, Kind::Load] {
            for it in self.items.iter().filter(|it| it.kind == kind) {
                for &t in &it.triangles {
                    slots[t as usize] = it.color as u8;
                }
            }
        }
        slots
    }

    /// The arrow of a load, `(tail, head)`, on the outer side of its faces. `None` for a
    /// support or a load with no force.
    pub fn arrow(&self, item: &Item) -> Option<([f32; 3], [f32; 3])> {
        let size = length(item.force);
        if item.kind != Kind::Load || size <= 0.0 || !size.is_finite() {
            return None;
        }
        let radius = self
            .mesh
            .bounds
            .map_or(1.0, |b| b.radius().max(f32::MIN_POSITIVE));
        let dir = item.force.map(|f| (f / size) as f32);
        let c = item.center.map(|x| x as f32);
        let step: [f32; 3] = std::array::from_fn(|k| dir[k] * ARROW * radius);
        // A push ends on the face. A pull or a shear starts on it.
        Some(match dot(item.force, item.outward) < 0.0 {
            true => (std::array::from_fn(|k| c[k] - step[k]), c),
            false => (c, std::array::from_fn(|k| c[k] + step[k])),
        })
    }

    /// The mean move of the triangle corners of an item from the mesh positions to
    /// `positions`, for example the deformed positions of the results. Zero when the item
    /// has no triangle.
    pub fn shift(&self, item: &Item, positions: &[[f32; 3]]) -> [f32; 3] {
        let corners = item
            .triangles
            .iter()
            .flat_map(|&t| self.mesh.triangles[t as usize]);
        let (mut sum, mut n) = ([0.0f64; 3], 0usize);
        for q in corners {
            let (Some(a), Some(b)) = (
                self.mesh.positions.get(q as usize),
                positions.get(q as usize),
            ) else {
                continue;
            };
            (0..3).for_each(|k| sum[k] += f64::from(b[k] - a[k]));
            n += 1;
        }
        sum.map(|x| (x / n.max(1) as f64) as f32)
    }

    /// True when `mesh` (for example the mesh of the results) has the positions and the
    /// triangles of the mesh of the input, so that the positions of one apply to the other.
    pub fn matches(&self, mesh: &RenderMesh) -> bool {
        let m = &self.mesh;
        let tolerance = m.bounds.map_or(0.0, |b| b.radius()) * 1e-5;
        m.triangles == mesh.triangles
            && m.positions.len() == mesh.positions.len()
            && m.positions
                .iter()
                .zip(&mesh.positions)
                .all(|(a, b)| (0..3).all(|k| (a[k] - b[k]).abs() <= tolerance))
    }

    /// The text of an item for the legend: what the input does, the size of the set, and
    /// the total force of a load.
    pub fn describe(&self, item: &Item) -> Vec<String> {
        let length = unit(&self.units, Quantity::Displacement);
        let force = unit(&self.units, Quantity::Force);
        let mut lines = item.actions.clone();
        let area = format_value(item.area);
        lines.push(format!("{} nodes, area {area} {length}2", item.nodes));
        if item.kind == Kind::Load {
            let f = item.force.map(format_value);
            lines.push(format!(
                "total force ({}, {}, {}) {force}",
                f[0], f[1], f[2]
            ));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_text(text: &str) -> Preview {
        build(&fh_core::inp::parse(text), &fh_core::inp::units(text))
    }

    fn bar() -> Preview {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../validation/cases/bar-pressure/job.inp"
        );
        from_text(
            &fh_core::inp::flatten(std::path::Path::new(path))
                .unwrap()
                .text,
        )
    }

    /// The surface triangles with each corner on the plane `x[axis] = at`.
    fn on_plane(p: &Preview, axis: usize, at: f32) -> Vec<u32> {
        let m = &p.mesh;
        let on = |t: &[u32; 3]| {
            t.iter()
                .all(|&q| (m.positions[q as usize][axis] - at).abs() < 1e-4)
        };
        (0..m.triangles.len() as u32)
            .filter(|&t| on(&m.triangles[t as usize]))
            .collect()
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-6 * (1.0 + b[k].abs()))
    }

    #[test]
    fn each_region_of_the_bar_has_the_faces_of_its_plane() {
        // validation/cases/bar-pressure: a bar of 100 x 20 x 10 mm, a support on each of 3
        // planes, and a pull of 100 MPa on the end face x = 100.
        let p = bar();
        assert_eq!(p.units, "SI-mm");
        let names: Vec<(&str, Kind)> = p.items.iter().map(|i| (i.name.as_str(), i.kind)).collect();
        assert_eq!(
            names,
            [
                ("SUPPORT", Kind::Support),
                ("SYM_Y", Kind::Support),
                ("SYM_Z", Kind::Support),
                ("LOAD", Kind::Load),
            ]
        );
        for (item, axis, at, area) in [
            (&p.items[0], 0, 0.0, 200.0),
            (&p.items[1], 1, 0.0, 1000.0),
            (&p.items[2], 2, 0.0, 2000.0),
            (&p.items[3], 0, 100.0, 200.0),
        ] {
            assert_eq!(item.triangles, on_plane(&p, axis, at), "{}", item.name);
            assert!(
                (item.area - area).abs() < 1e-6 * area,
                "{}: {}",
                item.name,
                item.area
            );
        }
        assert_eq!(p.items[0].actions, ["U1 = 0"]);
        assert_eq!(p.items[3].actions, ["pressure -100 MPa"]);
        let labels: Vec<&str> = p.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "SUPPORT: ux",
                "SYM_Y: uy",
                "SYM_Z: uz",
                "LOAD: -100 MPa pressure"
            ]
        );
        // The colours: 3 support colours, then the first load colour.
        let colors: Vec<usize> = p.items.iter().map(|i| i.color).collect();
        assert_eq!(colors, [1, 2, 3, 5]);
        let slots = p.slots();
        assert!(p.items[3].triangles.iter().all(|&t| slots[t as usize] == 5));
    }

    #[test]
    fn a_pull_points_out_of_the_face_and_its_total_is_the_reaction() {
        let p = bar();
        let load = &p.items[3];
        // -(-100 MPa) * 200 mm2 along the outward normal +x.
        assert!(close(load.force, [20000.0, 0.0, 0.0]), "{:?}", load.force);
        assert!(close(load.center, [100.0, 10.0, 5.0]), "{:?}", load.center);
        let (tail, head) = p.arrow(load).unwrap();
        assert_eq!(tail, [100.0, 10.0, 5.0]);
        assert!(
            head[0] > 100.0 && head[1] == 10.0 && head[2] == 5.0,
            "{head:?}"
        );
        assert_eq!(p.arrow(&p.items[0]), None);
        assert_eq!(
            p.describe(load).last().unwrap(),
            "total force (20000, 0, 0) N"
        );
    }

    /// 12 C3D10 elements: the box x 0 to 2, y and z 0 to 1, as 2 cubes of 6 tets. The set
    /// FIXED holds the nodes at x = 0, and the surface TOP the faces at z = 1. Nodes 1 to 12
    /// are the corners of the cubes, nodes 13 to 45 the midside nodes.
    const TWO_CUBES: &str = "** units: SI-mm\n*NODE\n\
        1, 0, 0, 0\n2, 1, 0, 0\n3, 1, 1, 0\n4, 0, 1, 0\n\
        5, 0, 0, 1\n6, 1, 0, 1\n7, 1, 1, 1\n8, 0, 1, 1\n\
        9, 2, 0, 0\n10, 2, 1, 0\n11, 2, 0, 1\n12, 2, 1, 1\n\
        13, 0.5, 0, 0\n14, 1, 0.5, 0\n15, 0.5, 0.5, 0\n16, 0.5, 0.5, 0.5\n17, 1, 0.5, 0.5\n\
        18, 1, 1, 0.5\n19, 0.5, 0, 0.5\n20, 1, 0, 0.5\n21, 1, 0.5, 1\n22, 0.5, 1, 0\n\
        23, 0, 0.5, 0\n24, 0.5, 1, 0.5\n25, 0, 1, 0.5\n26, 0, 0.5, 0.5\n27, 0.5, 1, 1\n\
        28, 0, 0, 0.5\n29, 0.5, 0, 1\n30, 0.5, 0.5, 1\n31, 0, 0.5, 1\n32, 1.5, 0, 0\n\
        33, 2, 0.5, 0\n34, 1.5, 0.5, 0\n35, 1.5, 0.5, 0.5\n36, 2, 0.5, 0.5\n37, 2, 1, 0.5\n\
        38, 1.5, 0, 0.5\n39, 2, 0, 0.5\n40, 2, 0.5, 1\n41, 1.5, 1, 0\n42, 1.5, 1, 0.5\n\
        43, 1.5, 1, 1\n44, 1.5, 0, 1\n45, 1.5, 0.5, 1\n\
        *ELEMENT, TYPE=C3D10\n\
        1, 1, 2, 3, 7, 13, 14, 15, 16, 17, 18\n2, 1, 6, 2, 7, 19, 20, 13, 16, 21, 17\n\
        3, 1, 3, 4, 7, 15, 22, 23, 16, 18, 24\n4, 1, 4, 8, 7, 23, 25, 26, 16, 24, 27\n\
        5, 1, 5, 6, 7, 28, 29, 19, 16, 30, 21\n6, 1, 8, 5, 7, 26, 31, 28, 16, 27, 30\n\
        7, 2, 9, 10, 12, 32, 33, 34, 35, 36, 37\n8, 2, 11, 9, 12, 38, 39, 32, 35, 40, 36\n\
        9, 2, 10, 3, 12, 34, 41, 14, 35, 37, 42\n10, 2, 3, 7, 12, 14, 18, 17, 35, 42, 43\n\
        11, 2, 6, 11, 12, 20, 44, 38, 35, 45, 40\n12, 2, 7, 6, 12, 17, 21, 20, 35, 43, 45\n\
        *NSET, NSET=FIXED\n1, 4, 5, 8, 23, 25, 26, 28, 31\n\
        *SURFACE, TYPE=ELEMENT, NAME=TOP\n5, S3\n6, S3\n11, S3\n12, S3\n";

    #[test]
    fn a_push_and_a_node_force_point_into_the_part() {
        // The box x 0 to 2, y and z 0 to 1. A pressure of 3 on the top faces (z = 1), a force
        // of -5 in z on each node of the end x = 2, and a support at x = 0.
        let text = format!(
            "{TWO_CUBES}*NSET, NSET=TIP\n9, 10, 11, 12, 33, 36, 37, 39, 40\n\
            *STEP\n*STATIC\n*BOUNDARY\nFIXED, 1, 3\n\
            *CLOAD\nTIP, 3, -5.0\n*DSLOAD\nTOP, P, 3.0\n*END STEP\n"
        );
        let p = from_text(&text);
        let [fixed, tip, top] = &p.items[..] else {
            panic!("{:?}", p.items);
        };
        assert_eq!((fixed.name.as_str(), fixed.kind), ("FIXED", Kind::Support));
        assert_eq!(fixed.actions, ["U1 to U3 = 0"]);
        assert_eq!(fixed.triangles, on_plane(&p, 0, 0.0));
        assert_eq!(tip.triangles, on_plane(&p, 0, 2.0));
        assert_eq!(top.triangles, on_plane(&p, 2, 1.0));
        assert_eq!(tip.actions, ["F3 = -5 N on each of 9 nodes"]);
        assert_eq!(fixed.label, "FIXED: ux uy uz");
        assert_eq!(tip.label, "TIP: 45 N, -z");
        assert_eq!(top.label, "TOP: 3 MPa pressure");
        assert!(close(tip.force, [0.0, 0.0, -45.0]), "{:?}", tip.force);
        // 3 MPa pushes down on 2 mm2 of the top face.
        assert!(close(top.force, [0.0, 0.0, -6.0]), "{:?}", top.force);
        let (tail, head) = p.arrow(top).unwrap();
        assert_eq!(head, [1.0, 0.5, 1.0]);
        assert!(tail[2] > 1.0, "{tail:?}");
        // A shear of the end face starts on the face.
        let (tail, head) = p.arrow(tip).unwrap();
        assert_eq!(tail, [2.0, 0.5, 0.5]);
        assert!(head[2] < 0.5 && head[0] == 2.0, "{head:?}");
    }
    #[test]
    fn a_temperature_support_has_no_length_unit() {
        // DOF 11 is the temperature, so the label must not show the length unit mm.
        let text = format!(
            "{TWO_CUBES}*STEP\n*HEAT TRANSFER, STEADY STATE\n*BOUNDARY\nFIXED, 11, 11, 100\n*END STEP\n"
        );
        let p = from_text(&text);
        assert_eq!(p.items[0].actions, ["Temperature = 100"]);
        assert_eq!(p.items[0].label, "FIXED: temperature = 100");
    }
}
