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
    /// The index of the model node (in model order) at each position.
    pub source_node: Vec<u32>,
    /// Surface triangles.
    pub triangles: Vec<[u32; 3]>,
    /// The index of the element (in model order) that owns each triangle.
    pub triangle_cell: Vec<u32>,
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
pub(crate) struct Face {
    pub(crate) corners: Vec<u32>,
    pub(crate) mids: Option<Vec<u32>>,
}

/// Local corner indices of each tet face, and the local indices of the mid-side nodes
/// (C3D10 order: 4 to 9 are the mids of edges 0-1, 1-2, 2-0, 0-3, 1-3, 2-3).
const TET_FACES: [([usize; 3], [usize; 3]); 4] = [
    ([0, 1, 2], [4, 5, 6]),
    ([0, 1, 3], [4, 8, 7]),
    ([1, 2, 3], [5, 9, 8]),
    ([2, 0, 3], [6, 7, 9]),
];

/// Face list of an element type, or `None` when the viewer cannot draw the type. Face `k`
/// of the list is the face `S<k+1>` of a `*SURFACE` or a `*DLOAD` in the input.
pub(crate) fn element_faces(element_type: &str, nodes: &[u32]) -> Option<Vec<Face>> {
    let t = element_type.to_ascii_uppercase();
    match t.as_str() {
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
    let points: Vec<[f64; 3]> = model.nodes.iter().map(|n| n.xyz).collect();
    let elements: Vec<Option<(&str, Vec<u32>)>> = model
        .elements
        .iter()
        .map(|el| {
            let nodes: Option<Vec<u32>> = el
                .nodes
                .iter()
                .map(|n| index.get(n).map(|&i| i as u32))
                .collect();
            Some((el.element_type.as_str(), nodes?))
        })
        .collect();
    build_indexed(&points, &elements)
}

/// The element type name of a VTK cell type, or `None` when the viewer cannot draw it.
pub fn vtk_element_type(cell_type: u8) -> Option<&'static str> {
    match cell_type {
        // VTK quadratic tetra has the C3D10 node order.
        24 => Some("C3D10"),
        _ => None,
    }
}

/// Build the render data for a VTU grid. Cell `k` of the grid is element `k`.
pub fn build_grid(grid: &fh_io::vtu::Grid) -> RenderMesh {
    let n = grid.points.len();
    let elements: Vec<Option<(&str, Vec<u32>)>> = grid
        .cell_types
        .iter()
        .zip(&grid.cells)
        .map(|(&t, cell)| {
            let valid = cell.iter().all(|&i| (i as usize) < n);
            Some((
                vtk_element_type(t).unwrap_or(""),
                valid.then(|| cell.clone())?,
            ))
        })
        .collect();
    build_indexed(&grid.points, &elements)
}

/// Build the render data from points and elements with 0-based point indices. `None` is
/// an element that names a missing point.
fn build_indexed(points: &[[f64; 3]], elements: &[Option<(&str, Vec<u32>)>]) -> RenderMesh {
    // Pass 1: gather every face and count how many elements share it.
    struct Entry {
        face: Face,
        count: u32,
        inside: [f64; 3],
        cell: u32,
    }
    let mut faces: HashMap<Vec<u32>, Entry> = HashMap::new();
    let mut order: Vec<Vec<u32>> = Vec::new();
    let mut skipped = 0;
    for (cell, el) in elements.iter().enumerate() {
        let Some((el_type, el_nodes)) = el else {
            skipped += 1;
            continue;
        };
        let Some(el_faces) = element_faces(el_type, el_nodes) else {
            skipped += 1;
            continue;
        };
        let mut centroid = [0.0; 3];
        for &n in el_nodes {
            let p = points[n as usize];
            for k in 0..3 {
                centroid[k] += p[k] / el_nodes.len() as f64;
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
                            cell: cell as u32,
                        },
                    );
                }
            }
        }
    }

    // Pass 2: emit the faces that one element owns.
    let mut used: HashMap<u32, u32> = HashMap::new();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut source_node: Vec<u32> = Vec::new();
    let mut slot = |id: u32, positions: &mut Vec<[f32; 3]>| -> u32 {
        *used.entry(id).or_insert_with(|| {
            let p = points[id as usize];
            positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            source_node.push(id);
            (positions.len() - 1) as u32
        })
    };
    let mut triangles = Vec::new();
    let mut triangle_cell = Vec::new();
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
        let pts: Vec<[f64; 3]> = c.iter().map(|&id| points[id as usize]).collect();
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
        triangle_cell.resize(triangles.len(), e.cell);
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
        source_node,
        triangles,
        triangle_cell,
        edges,
        feature_edges,
        node_count: points.len(),
        element_count: elements.len(),
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

    fn model(nodes: Vec<Node>, elements: Vec<Element>) -> Model {
        Model {
            nodes,
            elements,
            ..Model::default()
        }
    }

    /// C3D10 elements from the corner positions (ids 1, 2, ...) and the corners of each
    /// element. Each edge gets one midside node, with an id after the corners.
    fn tets(corners: &[[f64; 3]], list: &[[u32; 4]]) -> Model {
        let mut nodes: Vec<Node> = (corners.iter().enumerate())
            .map(|(i, p)| node(i as u32 + 1, p[0], p[1], p[2]))
            .collect();
        let mut mids = std::collections::HashMap::new();
        let mut elements = Vec::new();
        for (e, t) in list.iter().enumerate() {
            let mut ids = t.to_vec();
            for (a, b) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)] {
                let key = (t[a].min(t[b]), t[a].max(t[b]));
                let next = nodes.len() as u32 + 1;
                let id = *mids.entry(key).or_insert_with(|| {
                    let (p, q) = (corners[key.0 as usize - 1], corners[key.1 as usize - 1]);
                    let m: [f64; 3] = std::array::from_fn(|k| 0.5 * (p[k] + q[k]));
                    nodes.push(node(next, m[0], m[1], m[2]));
                    next
                });
                ids.push(id);
            }
            elements.push(Element {
                id: e as u32 + 1,
                element_type: "C3D10".into(),
                nodes: ids,
            });
        }
        model(nodes, elements)
    }

    fn tet10() -> Model {
        tets(
            &[[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [0., 0., 2.]],
            &[[1, 2, 3, 4]],
        )
    }

    /// 2 tets on the 2 sides of the plane x = 0. They share the face 1 3 4. The corner 5 is
    /// at `x5`.
    fn two_tets(x5: [f64; 3]) -> Model {
        let corners = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.], x5];
        tets(&corners, &[[1, 2, 3, 4], [1, 3, 5, 4]])
    }

    #[test]
    fn two_tets_hide_the_shared_face_and_the_flat_edges() {
        let m = build(&two_tets([-1., 0., 0.]));
        // 6 outer faces of 4 triangles each.
        assert_eq!(m.triangles.len(), 24);
        // 9 outer edges, each split in two by its midside node.
        assert_eq!(m.edges.len(), 18);
        // The faces at z = 0 and at y = 0 are flat across x = 0, so 2 of the 9 edges are not
        // features: 7 x 2 = 14 segments.
        assert_eq!(m.feature_edges.len(), 14);
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
        // The face of tet 2 at z = 0 turns by 10 degrees about the y axis: a bent plate.
        let a = 10f64.to_radians();
        let m = build(&two_tets([-a.cos(), 0., a.sin()]));
        // The bend edge is not a feature. The 14 outline segments are.
        assert_eq!(m.feature_edges.len(), 14);
    }

    #[test]
    fn unknown_element_and_missing_node_are_skipped() {
        let mut m = tet10();
        let tet = m.elements[0].clone();
        m.elements.push(Element {
            id: 2,
            element_type: "S4".into(),
            nodes: vec![1, 2, 3, 4],
        });
        let mut missing = Element { id: 3, ..tet };
        missing.nodes[9] = 99;
        m.elements.push(missing);
        let m = build(&m);
        assert_eq!(m.skipped_elements, 2);
        assert_eq!(m.triangles.len(), 16);
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
        let path = format!("{}/../../{name}", env!("CARGO_MANIFEST_DIR"));
        fh_inp::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn cantilever_fixture_is_a_closed_box() {
        let m = build(&fixture("validation/cases/cantilever-c3d10/mesh-8.inp"));
        assert_eq!(m.skipped_elements, 0);
        assert!(!m.triangles.is_empty());
        let b = m.bounds.unwrap();
        assert!(b.radius() > 0.0);
        // Each face of a closed C3D10 surface has 4 triangles and 3 edge segments of its own:
        // its 6 segments are shared with the next faces.
        assert_eq!(4 * m.edges.len(), 3 * m.triangles.len());
        // A box has 12 sharp edges, each split into segments by the mesh.
        assert!(m.feature_edges.len() >= 12);
        assert!(m.feature_edges.len() < m.edges.len());
    }

    #[test]
    fn c3d10_fixture_draws_without_skips() {
        let m = build(&fixture("fixtures/inp/m2-keywords.inp"));
        assert!(m.triangles.len() >= 16);
        assert!(m.element_count > m.skipped_elements);
    }
}
