//! The model before the solve (`docs/chat.md`): the surface faces of each support and each
//! load of a solver input, the direction and the total of each load, and the set names.
//! The input is the fixed revision that the run solves, so the page shows what the solver
//! gets.
//!
//! Plain functions with no GPU types, so unit tests run without a window.

use std::collections::{HashMap, HashSet};

use fh_inp::{Model, Target};

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
    /// The slot of the colour in `COLORS`.
    pub color: usize,
    /// The surface triangles of the set: indices into `mesh.triangles`.
    pub triangles: Vec<u32>,
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
    /// The name of the model in the page, for example the case folder.
    pub title: String,
    pub units: String,
    pub mesh: RenderMesh,
    pub items: Vec<Item>,
}

/// The units of an input from its line `** units: SI-mm`, or an empty text.
pub fn units(text: &str) -> String {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("** units:"))
        .map_or_else(String::new, |u| u.trim().to_owned())
}

/// A set of the input during the build: its nodes, or the element faces of a surface.
struct Draft {
    item: Item,
    surface: bool,
    nodes: HashSet<u32>,
    /// The node sets of the faces of each element of a surface, by element index.
    faces: HashMap<u32, Vec<HashSet<u32>>>,
    pressure: f64,
}

fn dofs(first: u8, last: u8) -> String {
    let name = |d: u8| match d {
        1..=3 => format!("U{d}"),
        _ => format!("DOF {d}"),
    };
    if last > first {
        format!("{} to {}", name(first), name(last))
    } else {
        name(first)
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

/// Read the supports (`*BOUNDARY`) and the loads (`*CLOAD`, `*DSLOAD`) of a model, and the
/// surface triangles of each set.
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
    // The draft of a set, new or found. A surface gets the nodes of each of its faces.
    let draft = |drafts: &mut Vec<Draft>, name: &str, kind: Kind, surface: bool| {
        let same = |d: &Draft| {
            d.item.kind == kind && d.surface == surface && d.item.name.eq_ignore_ascii_case(name)
        };
        if let Some(i) = drafts.iter().position(same) {
            return i;
        }
        let mut d = Draft {
            item: Item {
                name: name.to_owned(),
                kind,
                actions: Vec::new(),
                color: 0,
                triangles: Vec::new(),
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
        };
        let faces = model.element_surface(name).filter(|_| surface);
        for &(id, face) in faces.map_or(&[][..], |s| &s.faces) {
            let Some(&e) = element.get(&id) else {
                continue;
            };
            let all = mesh::element_faces(&model.elements[e].element_type, &element_nodes(e));
            let Some(f) = all.and_then(|f| f.into_iter().nth(usize::from(face).wrapping_sub(1)))
            else {
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
        drafts.push(d);
        drafts.len() - 1
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
    let steps = || model.steps.iter();
    for b in model
        .boundaries
        .iter()
        .chain(steps().flat_map(|s| &s.boundaries))
    {
        let (name, nodes) = nodes_of(&b.target);
        let i = draft(&mut drafts, &name, Kind::Support, false);
        let value = if b.value == 0.0 {
            "0".to_owned()
        } else {
            format!("{} {length_unit}", b.value)
        };
        drafts[i]
            .item
            .actions
            .push(format!("{} = {value}", dofs(b.first_dof, b.last_dof)));
        drafts[i].nodes = nodes;
    }
    for c in steps().flat_map(|s| &s.cloads) {
        let (name, nodes) = nodes_of(&c.target);
        let i = draft(&mut drafts, &name, Kind::Load, false);
        let n = nodes.len();
        if (1..=3).contains(&c.dof) {
            drafts[i].item.force[usize::from(c.dof) - 1] += c.magnitude * n as f64;
        }
        let on = if n == 1 {
            "1 node".to_owned()
        } else {
            format!("each of {n} nodes")
        };
        let (dof, m) = (c.dof, c.magnitude);
        drafts[i]
            .item
            .actions
            .push(format!("F{dof} = {m} {force} on {on}"));
        drafts[i].nodes = nodes;
    }
    for d in steps().flat_map(|s| &s.dsloads) {
        let i = draft(&mut drafts, &d.surface, Kind::Load, true);
        drafts[i].pressure += d.pressure;
        let p = d.pressure;
        drafts[i]
            .item
            .actions
            .push(format!("pressure {p} {stress}"));
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
                if dot(area, sub(centroids[cell as usize], a)) > 0.0 {
                    area = area.map(|x| -x);
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
            let count = match it.kind {
                Kind::Support => &mut supports,
                Kind::Load => &mut loads,
            };
            it.color = 1 + 4 * usize::from(it.kind == Kind::Load) + *count % 4;
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

    fn bar() -> Preview {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../validation/cases/bar-pressure/bar.inp"
        );
        let text = std::fs::read_to_string(path).unwrap();
        build(&fh_inp::parse(&text).unwrap(), &units(&text))
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

    #[test]
    fn a_push_and_a_node_force_point_into_the_part() {
        // 2 hex elements: x 0 to 2, y and z 0 to 1. A pressure of 3 on the top faces (S2,
        // z = 1), a force of -5 in z on each node of the end x = 2, and a support at x = 0.
        let text = "** units: SI-mm\n*NODE\n\
            1, 0, 0, 0\n2, 1, 0, 0\n3, 1, 1, 0\n4, 0, 1, 0\n\
            5, 0, 0, 1\n6, 1, 0, 1\n7, 1, 1, 1\n8, 0, 1, 1\n\
            9, 2, 0, 0\n10, 2, 1, 0\n11, 2, 0, 1\n12, 2, 1, 1\n\
            *ELEMENT, TYPE=C3D8\n1, 1, 2, 3, 4, 5, 6, 7, 8\n2, 2, 9, 10, 3, 6, 11, 12, 7\n\
            *NSET, NSET=FIXED\n1, 4, 5, 8\n*NSET, NSET=TIP\n9, 10, 11, 12\n\
            *SURFACE, TYPE=ELEMENT, NAME=TOP\n1, S2\n2, S2\n\
            *STEP\n*STATIC\n*BOUNDARY\nFIXED, 1, 3\n\
            *CLOAD\nTIP, 3, -5.0\n*DSLOAD\nTOP, P, 3.0\n*END STEP\n";
        let p = build(&fh_inp::parse(text).unwrap(), &units(text));
        let [fixed, tip, top] = &p.items[..] else {
            panic!("{:?}", p.items);
        };
        assert_eq!((fixed.name.as_str(), fixed.kind), ("FIXED", Kind::Support));
        assert_eq!(fixed.actions, ["U1 to U3 = 0"]);
        assert_eq!(fixed.triangles, on_plane(&p, 0, 0.0));
        assert_eq!(tip.triangles, on_plane(&p, 0, 2.0));
        assert_eq!(top.triangles, on_plane(&p, 2, 1.0));
        assert_eq!(tip.actions, ["F3 = -5 N on each of 4 nodes"]);
        assert!(close(tip.force, [0.0, 0.0, -20.0]), "{:?}", tip.force);
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
}
