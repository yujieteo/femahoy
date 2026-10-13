//! Mesh to render data: surface faces, element edges, feature edges and the bounding box.
//!
//! Plain functions with no GPU types, so unit tests run without a window.

use std::collections::HashMap;

use fh_inp::Model;

/// Angle in degrees between two surface normals above which an edge is a feature edge.
pub const FEATURE_ANGLE_DEG: f64 = 30.0;

/// Render data for one model. All index lists point into `positions`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderMesh {
    /// One position per model node that a surface face uses, in input order.
    pub positions: Vec<[f32; 3]>,
    /// Surface triangles.
    pub triangles: Vec<[u32; 3]>,
    /// Element edges on the surface (line segments).
    pub edges: Vec<[u32; 2]>,
    /// Feature edges: the outline of the part (line segments).
    pub feature_edges: Vec<[u32; 2]>,
    /// Number of nodes in the model.
    pub node_count: usize,
    /// Number of elements in the model.
    pub element_count: usize,
    /// Elements with a type the viewer cannot draw.
    pub skipped_elements: usize,
    /// Smallest and largest coordinate, or `None` when the model has no node.
    pub bounds: Option<Bounds>,
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn center(&self) -> [f32; 3] {
        std::array::from_fn(|i| 0.5 * (self.min[i] + self.max[i]))
    }

    /// Half of the box diagonal. Zero for a single point.
    pub fn radius(&self) -> f32 {
        let d: [f32; 3] = std::array::from_fn(|i| self.max[i] - self.min[i]);
        0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

/// A face of one element. `corners` are the corner nodes in element-local order.
/// `mids` holds the mid-side node of each corner edge, when the element has them.
struct Face {
    corners: Vec<u32>,
    mids: Option<Vec<u32>>,
}

/// Local corner indices of each face, ordered so that the face is closed.
const HEX_FACES: [[usize; 4]; 6] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

/// Local corner indices of each tet face, and the local indices of the mid-side nodes
/// (C3D10 order: 4 to 9 are the mids of edges 0-1, 1-2, 2-0, 0-3, 1-3, 2-3).
const TET_FACES: [([usize; 3], [usize; 3]); 4] = [
    ([0, 1, 2], [4, 5, 6]),
    ([0, 1, 3], [4, 8, 7]),
    ([1, 2, 3], [5, 9, 8]),
    ([2, 0, 3], [6, 7, 9]),
];

/// Face list of an element type, or `None` when the viewer cannot draw the type.
fn element_faces(element_type: &str, nodes: &[u32]) -> Option<Vec<Face>> {
    let t = element_type.to_ascii_uppercase();
    match t.as_str() {
        "C3D8" | "C3D8R" | "C3D8I" if nodes.len() >= 8 => Some(
            HEX_FACES
                .iter()
                .map(|f| Face {
                    corners: f.iter().map(|&i| nodes[i]).collect(),
                    mids: None,
                })
                .collect(),
        ),
        "C3D4" if nodes.len() >= 4 => Some(
            TET_FACES
                .iter()
                .map(|(c, _)| Face {
                    corners: c.iter().map(|&i| nodes[i]).collect(),
                    mids: None,
                })
                .collect(),
        ),
        "C3D10" if nodes.len() >= 10 => Some(
            TET_FACES
                .iter()
                .map(|(c, m)| Face {
                    corners: c.iter().map(|&i| nodes[i]).collect(),
                    mids: Some(m.iter().map(|&i| nodes[i]).collect()),
                })
                .collect(),
        ),
        _ => None,
    }
}

fn sorted_key(corners: &[u32]) -> Vec<u32> {
    let mut k = corners.to_vec();
    k.sort_unstable();
    k
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f64; 3]) -> Option<[f64; 3]> {
    let n = dot(a, a).sqrt();
    (n > 0.0 && n.is_finite()).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// Outward unit normal of a face, or `None` for a degenerate face. `inside` is any
/// point in the element; the normal is flipped when it points toward that point.
fn face_normal(pts: &[[f64; 3]], inside: [f64; 3]) -> Option<[f64; 3]> {
    let n = if pts.len() == 4 {
        cross(sub(pts[2], pts[0]), sub(pts[3], pts[1]))
    } else {
        cross(sub(pts[1], pts[0]), sub(pts[2], pts[0]))
    };
    let n = normalize(n)?;
    let to_inside = sub(inside, pts[0]);
    Some(if dot(n, to_inside) > 0.0 {
        [-n[0], -n[1], -n[2]]
    } else {
        n
    })
}

/// Build the render data for a model.
///
/// A face that belongs to one element only is a surface face. Elements with an unknown
/// type, or with a node number that is not in the model, are skipped and counted.
pub fn build(model: &Model) -> RenderMesh {
    let index: HashMap<u32, usize> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();

    // Pass 1: gather every face and count how many elements share it.
    struct Entry {
        face: Face,
        count: u32,
        inside: [f64; 3],
    }
    let mut faces: HashMap<Vec<u32>, Entry> = HashMap::new();
    let mut order: Vec<Vec<u32>> = Vec::new();
    let mut skipped = 0;
    for el in &model.elements {
        let valid = el.nodes.iter().all(|n| index.contains_key(n));
        let Some(el_faces) = valid
            .then(|| element_faces(&el.element_type, &el.nodes))
            .flatten()
        else {
            skipped += 1;
            continue;
        };
        let mut centroid = [0.0; 3];
        for n in &el.nodes {
            let p = model.nodes[index[n]].xyz;
            for k in 0..3 {
                centroid[k] += p[k] / el.nodes.len() as f64;
            }
        }
        for face in el_faces {
            let key = sorted_key(&face.corners);
            match faces.get_mut(&key) {
                Some(e) => e.count += 1,
                None => {
                    order.push(key.clone());
                    faces.insert(
                        key,
                        Entry {
                            face,
                            count: 1,
                            inside: centroid,
                        },
                    );
                }
            }
        }
    }

    // Pass 2: emit the faces that one element owns.
    let mut used: HashMap<u32, u32> = HashMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut slot = |id: u32, positions: &mut Vec<[f32; 3]>| -> u32 {
        *used.entry(id).or_insert_with(|| {
            let p = model.nodes[index[&id]].xyz;
            positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            (positions.len() - 1) as u32
        })
    };
    let mut triangles = Vec::new();
    let mut edges: Vec<[u32; 2]> = Vec::new();
    // Corner edge (sorted node pair) to the outward normals of the surface faces on it.
    let mut edge_normals: HashMap<(u32, u32), Vec<[f64; 3]>> = HashMap::new();
    let mut edge_order: Vec<(u32, u32)> = Vec::new();
    let mut edge_path: HashMap<(u32, u32), Vec<u32>> = HashMap::new();

    for key in &order {
        let e = &faces[key];
        if e.count != 1 {
            continue;
        }
        let c = &e.face.corners;
        let pts: Vec<[f64; 3]> = c.iter().map(|id| model.nodes[index[id]].xyz).collect();
        let normal = face_normal(&pts, e.inside);
        let ci: Vec<u32> = c.iter().map(|&id| slot(id, &mut positions)).collect();
        match (&e.face.mids, c.len()) {
            (Some(m), 3) => {
                let mi: Vec<u32> = m.iter().map(|&id| slot(id, &mut positions)).collect();
                triangles.push([ci[0], mi[0], mi[2]]);
                triangles.push([mi[0], ci[1], mi[1]]);
                triangles.push([mi[2], mi[1], ci[2]]);
                triangles.push([mi[0], mi[1], mi[2]]);
            }
            (_, 3) => triangles.push([ci[0], ci[1], ci[2]]),
            _ => {
                triangles.push([ci[0], ci[1], ci[2]]);
                triangles.push([ci[0], ci[2], ci[3]]);
            }
        }
        for k in 0..c.len() {
            let next = (k + 1) % c.len();
            let (a, b) = (c[k], c[next]);
            let pair = (a.min(b), a.max(b));
            let path = match &e.face.mids {
                Some(m) => vec![ci[k], slot(m[k], &mut positions), ci[next]],
                None => vec![ci[k], ci[next]],
            };
            if let std::collections::hash_map::Entry::Vacant(v) = edge_path.entry(pair) {
                for w in path.windows(2) {
                    edges.push([w[0], w[1]]);
                }
                v.insert(path);
                edge_order.push(pair);
            }
            let list = edge_normals.entry(pair).or_default();
            match normal {
                Some(n) => list.push(n),
                // A degenerate face has no normal. Mark the edge as a feature.
                None => list.push([0.0; 3]),
            }
        }
    }

    let cos_limit = FEATURE_ANGLE_DEG.to_radians().cos();
    let mut feature_edges = Vec::new();
    for pair in &edge_order {
        let n = &edge_normals[pair];
        let feature = n.len() != 2 || n.contains(&[0.0; 3]) || dot(n[0], n[1]) < cos_limit;
        if feature {
            for w in edge_path[pair].windows(2) {
                feature_edges.push([w[0], w[1]]);
            }
        }
    }

    RenderMesh {
        bounds: bounds(&positions),
        positions,
        triangles,
        edges,
        feature_edges,
        node_count: model.nodes.len(),
        element_count: model.elements.len(),
        skipped_elements: skipped,
    }
}

/// Axis-aligned bounding box of a point list, or `None` for an empty list.
pub fn bounds(points: &[[f32; 3]]) -> Option<Bounds> {
    let first = *points.first()?;
    let mut b = Bounds {
        min: first,
        max: first,
    };
    for p in points {
        b.min = std::array::from_fn(|k| b.min[k].min(p[k]));
        b.max = std::array::from_fn(|k| b.max[k].max(p[k]));
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fh_inp::{Element, Node};

    fn node(id: u32, x: f64, y: f64, z: f64) -> Node {
        Node { id, xyz: [x, y, z] }
    }

    fn unit_cube_nodes() -> Vec<Node> {
        let c = [
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 1., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [1., 0., 1.],
            [1., 1., 1.],
            [0., 1., 1.],
        ];
        c.iter()
            .enumerate()
            .map(|(i, p)| node(i as u32 + 1, p[0], p[1], p[2]))
            .collect()
    }

    fn hex(id: u32, nodes: [u32; 8]) -> Element {
        Element {
            id,
            element_type: "C3D8".into(),
            nodes: nodes.to_vec(),
        }
    }

    fn model(nodes: Vec<Node>, elements: Vec<Element>) -> Model {
        Model {
            nodes,
            elements,
            ..Model::default()
        }
    }

    #[test]
    fn single_hex_has_six_quads_and_twelve_edges() {
        let m = build(&model(
            unit_cube_nodes(),
            vec![hex(1, [1, 2, 3, 4, 5, 6, 7, 8])],
        ));
        assert_eq!(m.triangles.len(), 12);
        assert_eq!(m.edges.len(), 12);
        assert_eq!(m.feature_edges.len(), 12);
        assert_eq!((m.node_count, m.element_count), (8, 1));
    }

    #[test]
    fn two_hexes_hide_the_shared_face_and_the_flat_edges() {
        let mut nodes = unit_cube_nodes();
        nodes.extend([
            node(9, 2., 0., 0.),
            node(10, 2., 1., 0.),
            node(11, 2., 0., 1.),
            node(12, 2., 1., 1.),
        ]);
        let els = vec![
            hex(1, [1, 2, 3, 4, 5, 6, 7, 8]),
            hex(2, [2, 9, 10, 3, 6, 11, 12, 7]),
        ];
        let m = build(&model(nodes, els));
        // 10 outer quads.
        assert_eq!(m.triangles.len(), 20);
        // A closed quad surface with 10 faces has 10 x 4 / 2 = 20 edges.
        assert_eq!(m.edges.len(), 20);
        // The outline is the box edges. The 4 edges along x are split in two by the
        // middle nodes: 8 + 4 + 4 = 16 segments.
        assert_eq!(m.feature_edges.len(), 16);
    }

    #[test]
    fn single_tet_has_four_faces() {
        let nodes = vec![
            node(1, 0., 0., 0.),
            node(2, 1., 0., 0.),
            node(3, 0., 1., 0.),
            node(4, 0., 0., 1.),
        ];
        let el = Element {
            id: 1,
            element_type: "C3D4".into(),
            nodes: vec![1, 2, 3, 4],
        };
        let m = build(&model(nodes, vec![el]));
        assert_eq!(m.triangles.len(), 4);
        assert_eq!(m.edges.len(), 6);
        assert_eq!(m.feature_edges.len(), 6);
    }

    fn tet10() -> Model {
        let c = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [0., 0., 2.]];
        let mid =
            |a: usize, b: usize| -> [f64; 3] { std::array::from_fn(|k| 0.5 * (c[a][k] + c[b][k])) };
        let pairs = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
        let mut nodes: Vec<Node> = c
            .iter()
            .enumerate()
            .map(|(i, p)| node(i as u32 + 1, p[0], p[1], p[2]))
            .collect();
        for (i, (a, b)) in pairs.iter().enumerate() {
            let p = mid(*a, *b);
            nodes.push(node(i as u32 + 5, p[0], p[1], p[2]));
        }
        let el = Element {
            id: 1,
            element_type: "C3D10".into(),
            nodes: (1..=10).collect(),
        };
        model(nodes, vec![el])
    }

    #[test]
    fn tet10_splits_each_face_in_four_and_each_edge_in_two() {
        let m = build(&tet10());
        assert_eq!(m.triangles.len(), 16);
        assert_eq!(m.edges.len(), 12);
        assert_eq!(m.feature_edges.len(), 12);
        assert_eq!(m.positions.len(), 10);
    }

    #[test]
    fn smooth_fold_below_threshold_is_not_a_feature() {
        // Two quads that meet at 10 degrees: a bent plate, one element each side.
        let a = 10f64.to_radians();
        let nodes = vec![
            node(1, 0., 0., 0.),
            node(2, 1., 0., 0.),
            node(3, 1., 1., 0.),
            node(4, 0., 1., 0.),
            node(5, 0., 0., 0.2),
            node(6, 1., 0., 0.2),
            node(7, 1., 1., 0.2),
            node(8, 0., 1., 0.2),
            node(9, 1. + a.cos(), 0., a.sin()),
            node(10, 1. + a.cos(), 1., a.sin()),
            node(11, 1. + a.cos(), 0., 0.2 + a.sin()),
            node(12, 1. + a.cos(), 1., 0.2 + a.sin()),
        ];
        let els = vec![
            hex(1, [1, 2, 3, 4, 5, 6, 7, 8]),
            hex(2, [2, 9, 10, 3, 6, 11, 12, 7]),
        ];
        let m = build(&model(nodes, els));
        // The two slabs share a face and the outer surface bends by 10 degrees across
        // the shared plane. The two bend edges are not features. The 16 outline
        // segments are.
        assert_eq!(m.feature_edges.len(), 16);
    }

    #[test]
    fn unknown_element_and_missing_node_are_skipped() {
        let els = vec![
            Element {
                id: 1,
                element_type: "S4".into(),
                nodes: vec![1, 2, 3, 4],
            },
            hex(2, [1, 2, 3, 4, 5, 6, 7, 99]),
            hex(3, [1, 2, 3, 4, 5, 6, 7, 8]),
        ];
        let m = build(&model(unit_cube_nodes(), els));
        assert_eq!(m.skipped_elements, 2);
        assert_eq!(m.triangles.len(), 12);
        assert_eq!(m.element_count, 3);
    }

    #[test]
    fn bounds_cover_all_points() {
        let b = bounds(&[[1., -2., 3.], [-1., 5., 0.]]).unwrap();
        assert_eq!(b.min, [-1., -2., 0.]);
        assert_eq!(b.max, [1., 5., 3.]);
        assert_eq!(b.center(), [0., 1.5, 1.5]);
        assert!(bounds(&[]).is_none());
    }

    #[test]
    fn empty_model_has_no_bounds() {
        let m = build(&Model::default());
        assert!(m.bounds.is_none());
        assert!(m.triangles.is_empty());
    }

    fn fixture(name: &str) -> Model {
        let path = format!("{}/../../fixtures/inp/{name}", env!("CARGO_MANIFEST_DIR"));
        fh_inp::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn cantilever_fixture_is_a_closed_box() {
        let m = build(&fixture("cantilever.inp"));
        assert_eq!(m.skipped_elements, 0);
        assert!(!m.triangles.is_empty());
        let b = m.bounds.unwrap();
        assert!(b.radius() > 0.0);
        // A closed quad surface has twice as many edges as quads, and two triangles
        // make one quad.
        assert_eq!(m.edges.len(), m.triangles.len());
        // A box has 12 sharp edges, each split into segments by the mesh.
        assert!(m.feature_edges.len() >= 12);
        assert!(m.feature_edges.len() < m.edges.len());
    }

    #[test]
    fn c3d10_fixture_draws_without_skips() {
        let m = build(&fixture("m2-keywords.inp"));
        assert!(m.triangles.len() >= 16);
        assert!(m.element_count > m.skipped_elements);
    }
}
