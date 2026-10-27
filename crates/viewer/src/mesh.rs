//! Mesh to render data: surface faces, element edges, feature edges and the bounding box.
//!
//! Plain functions with no GPU types, so unit tests run without a window.

use std::collections::HashMap;

use fh_core::inp::Model;
use fh_core::vtu::{
    VTK_HEXAHEDRON, VTK_QUAD, VTK_QUADRATIC_HEXAHEDRON, VTK_QUADRATIC_TETRA, VTK_QUADRATIC_WEDGE,
    VTK_TETRA, VTK_TRIANGLE, VTK_WEDGE,
};

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

/// The faces of an element shape: for each face, the local indices of its corner nodes and of
/// the mid-side node of each corner edge (`mids[k]` is on the edge from `corners[k]` to the next
/// corner). A shape with 1 face is a shell. The node order is the CalculiX order.
type Faces = &'static [(&'static [usize], &'static [usize])];

const TET4: Faces = &[
    (&[0, 1, 2], &[]),
    (&[0, 1, 3], &[]),
    (&[1, 2, 3], &[]),
    (&[2, 0, 3], &[]),
];
const TET10: Faces = &[
    (&[0, 1, 2], &[4, 5, 6]),
    (&[0, 1, 3], &[4, 8, 7]),
    (&[1, 2, 3], &[5, 9, 8]),
    (&[2, 0, 3], &[6, 7, 9]),
];
const HEX8: Faces = &[
    (&[0, 1, 2, 3], &[]),
    (&[4, 7, 6, 5], &[]),
    (&[0, 4, 5, 1], &[]),
    (&[1, 5, 6, 2], &[]),
    (&[2, 6, 7, 3], &[]),
    (&[3, 7, 4, 0], &[]),
];
const HEX20: Faces = &[
    (&[0, 1, 2, 3], &[8, 9, 10, 11]),
    (&[4, 7, 6, 5], &[15, 14, 13, 12]),
    (&[0, 4, 5, 1], &[16, 12, 17, 8]),
    (&[1, 5, 6, 2], &[17, 13, 18, 9]),
    (&[2, 6, 7, 3], &[18, 14, 19, 10]),
    (&[3, 7, 4, 0], &[19, 15, 16, 11]),
];
const WEDGE6: Faces = &[
    (&[0, 1, 2], &[]),
    (&[3, 4, 5], &[]),
    (&[0, 1, 4, 3], &[]),
    (&[1, 2, 5, 4], &[]),
    (&[2, 0, 3, 5], &[]),
];
const WEDGE15: Faces = &[
    (&[0, 1, 2], &[6, 7, 8]),
    (&[3, 4, 5], &[9, 10, 11]),
    (&[0, 1, 4, 3], &[6, 13, 9, 12]),
    (&[1, 2, 5, 4], &[7, 14, 10, 13]),
    (&[2, 0, 3, 5], &[8, 12, 11, 14]),
];
const TRI3: Faces = &[(&[0, 1, 2], &[])];
const TRI6: Faces = &[(&[0, 1, 2], &[3, 4, 5])];
const QUAD4: Faces = &[(&[0, 1, 2, 3], &[])];
const QUAD8: Faces = &[(&[0, 1, 2, 3], &[4, 5, 6, 7])];

/// The number of nodes and the faces of the element types of CalculiX that the viewer draws
/// (solids, heat transfer solids and shells), or `None` for another type. A letter at the end
/// of the name (`C3D8R`, `S8R`) is an option of the type and does not change the shape.
fn shape(element_type: &str) -> Option<(usize, Faces)> {
    let upper = element_type.to_ascii_uppercase();
    let name = (upper.strip_prefix('D')).filter(|n| n.starts_with("C3D"));
    let name = name.unwrap_or(&upper).trim_end_matches(['R', 'I', 'T']);
    Some(match name {
        "C3D4" => (4, TET4),
        "C3D10" => (10, TET10),
        "C3D8" => (8, HEX8),
        "C3D20" => (20, HEX20),
        "C3D6" => (6, WEDGE6),
        "C3D15" => (15, WEDGE15),
        "S3" => (3, TRI3),
        "S6" => (6, TRI6),
        "S4" => (4, QUAD4),
        "S8" | "S9" => (8, QUAD8),
        _ => return None,
    })
}

/// Face list of an element type, or `None` when the viewer cannot draw the type. Face `k`
/// of the list is the face `S<k+1>` of a `*SURFACE` or a `*DLOAD` in the input.
pub(crate) fn element_faces(element_type: &str, nodes: &[u32]) -> Option<Vec<Face>> {
    let (count, faces) = shape(element_type)?;
    (nodes.len() >= count).then(|| {
        let at = |list: &[usize]| list.iter().map(|&i| nodes[i]).collect::<Vec<_>>();
        faces
            .iter()
            .map(|(c, m)| Face {
                corners: at(c),
                mids: Some(at(m)).filter(|m| !m.is_empty()),
            })
            .collect()
    })
}

/// The face `S<number>` of an element. A shell has 1 drawn face for its faces `S1` and `S2`.
/// Its faces `S3` and up are its edges: edge `S3` joins corner 1 and corner 2.
pub(crate) fn surface_face(element_type: &str, nodes: &[u32], number: u8) -> Option<Face> {
    let mut faces = element_faces(element_type, nodes)?;
    let k = usize::from(number).checked_sub(1)?;
    match faces.len() {
        1 if k <= 1 => faces.pop(),
        1 => {
            let face = faces.pop()?;
            let (j, n) = (k - 2, face.corners.len());
            (j < n).then(|| Face {
                corners: vec![face.corners[j], face.corners[(j + 1) % n]],
                mids: face.mids.map(|m| vec![m[j]]),
            })
        }
        _ if k < faces.len() => Some(faces.swap_remove(k)),
        _ => None,
    }
}

/// The sorted corners of a face (3 or 4), with `u32::MAX` after 3 corners.
fn sorted_key(corners: &[u32]) -> [u32; 4] {
    let mut k = [u32::MAX; 4];
    k[..corners.len()].copy_from_slice(corners);
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
/// point in the element; the normal is flipped when it points toward that point. A point in
/// the plane of the face (a shell) does not flip the normal.
fn face_normal(pts: &[[f64; 3]], inside: [f64; 3]) -> Option<[f64; 3]> {
    let n = if pts.len() == 4 {
        cross(sub(pts[2], pts[0]), sub(pts[3], pts[1]))
    } else {
        cross(sub(pts[1], pts[0]), sub(pts[2], pts[0]))
    };
    let n = normalize(n)?;
    let to_inside = sub(inside, pts[0]);
    Some(
        if dot(n, to_inside) > 1e-9 * dot(to_inside, to_inside).sqrt() {
            [-n[0], -n[1], -n[2]]
        } else {
            n
        },
    )
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

/// The element type name of a VTK cell type, or `None` when the viewer cannot draw it. The
/// results file has the node order of CalculiX for each type that it writes.
pub fn vtk_element_type(cell_type: u8) -> Option<&'static str> {
    match cell_type {
        VTK_TETRA => Some("C3D4"),
        VTK_HEXAHEDRON => Some("C3D8"),
        VTK_WEDGE => Some("C3D6"),
        VTK_QUADRATIC_TETRA => Some("C3D10"),
        VTK_QUADRATIC_HEXAHEDRON => Some("C3D20"),
        VTK_QUADRATIC_WEDGE => Some("C3D15"),
        VTK_TRIANGLE => Some("S3"),
        VTK_QUAD => Some("S4"),
        _ => None,
    }
}

/// Build the render data for a VTU grid. Cell `k` of the grid is element `k`.
pub fn build_grid(grid: &fh_core::vtu::Grid) -> RenderMesh {
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
///
/// The work data has fixed-size keys and indices, not a list for each face and edge. Reason:
/// the 33416 shells of an A320 fuselage expand to 299903 points in the results, and a list for
/// each face and edge took 120 MB of a phone browser for a mesh of 17 MB.
fn build_indexed(points: &[[f64; 3]], elements: &[Option<(&str, Vec<u32>)>]) -> RenderMesh {
    // Pass 1: gather every face and count how many elements share it. A face is its element
    // and its number in the element, in the order first seen.
    struct Seen {
        cell: u32,
        face: u8,
        count: u32,
    }
    let mut index: HashMap<[u32; 4], u32> = HashMap::new();
    let mut seen: Vec<Seen> = Vec::new();
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
        for (k, face) in el_faces.iter().enumerate() {
            match index.entry(sorted_key(&face.corners)) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    seen[*e.get() as usize].count += 1
                }
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(seen.len() as u32);
                    seen.push(Seen {
                        cell: cell as u32,
                        face: k as u8,
                        count: 1,
                    });
                }
            }
        }
    }
    drop(index);

    // Pass 2: emit the faces that one element owns.
    let mut used: Vec<u32> = vec![u32::MAX; points.len()];
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut source_node: Vec<u32> = Vec::new();
    let mut slot = |id: u32, positions: &mut Vec<[f32; 3]>| -> u32 {
        let at = &mut used[id as usize];
        if *at == u32::MAX {
            let p = points[id as usize];
            positions.push([p[0] as f32, p[1] as f32, p[2] as f32]);
            source_node.push(id);
            *at = (positions.len() - 1) as u32;
        }
        *at
    };
    let mut triangles = Vec::new();
    let mut triangle_cell = Vec::new();
    let mut edges: Vec<[u32; 2]> = Vec::new();
    // A corner edge (sorted node pair) on the surface: its path of positions (2, or 3 with
    // the mid-side node) and the outward normals of the surface faces on it.
    struct Edge {
        path: [u32; 3],
        len: u8,
        normals: u32,
        first: [[f64; 3]; 2],
        /// A face on the edge has no normal.
        degenerate: bool,
    }
    let mut edge_index: HashMap<(u32, u32), u32> = HashMap::new();
    let mut edge_list: Vec<Edge> = Vec::new();

    for s in seen.iter().filter(|s| s.count == 1) {
        let Some((el_type, el_nodes)) = &elements[s.cell as usize] else {
            continue;
        };
        let Some(face) =
            element_faces(el_type, el_nodes).map(|mut f| f.swap_remove(usize::from(s.face)))
        else {
            continue;
        };
        let mut inside = [0.0; 3];
        for &n in el_nodes {
            let p = points[n as usize];
            for k in 0..3 {
                inside[k] += p[k] / el_nodes.len() as f64;
            }
        }
        let c = &face.corners;
        let pts: Vec<[f64; 3]> = c.iter().map(|&id| points[id as usize]).collect();
        let normal = face_normal(&pts, inside);
        let ci: Vec<u32> = c.iter().map(|&id| slot(id, &mut positions)).collect();
        match (&face.mids, c.len()) {
            (Some(m), 3) => {
                let mi: Vec<u32> = m.iter().map(|&id| slot(id, &mut positions)).collect();
                triangles.push([ci[0], mi[0], mi[2]]);
                triangles.push([mi[0], ci[1], mi[1]]);
                triangles.push([mi[2], mi[1], ci[2]]);
                triangles.push([mi[0], mi[1], mi[2]]);
            }
            (Some(m), _) => {
                let mi: Vec<u32> = m.iter().map(|&id| slot(id, &mut positions)).collect();
                triangles.push([ci[0], mi[0], mi[3]]);
                triangles.push([ci[1], mi[1], mi[0]]);
                triangles.push([ci[2], mi[2], mi[1]]);
                triangles.push([ci[3], mi[3], mi[2]]);
                triangles.push([mi[0], mi[1], mi[2]]);
                triangles.push([mi[0], mi[2], mi[3]]);
            }
            (_, 3) => triangles.push([ci[0], ci[1], ci[2]]),
            _ => {
                triangles.push([ci[0], ci[1], ci[2]]);
                triangles.push([ci[0], ci[2], ci[3]]);
            }
        }
        triangle_cell.resize(triangles.len(), s.cell);
        // A degenerate face has no normal. It marks the edge as a feature.
        let normal = normal.unwrap_or([0.0; 3]);
        for k in 0..c.len() {
            let next = (k + 1) % c.len();
            let (a, b) = (c[k], c[next]);
            let pair = (a.min(b), a.max(b));
            let i = *edge_index.entry(pair).or_insert_with(|| {
                let (path, len) = match &face.mids {
                    Some(m) => ([ci[k], slot(m[k], &mut positions), ci[next]], 3),
                    None => ([ci[k], ci[next], 0], 2),
                };
                for w in path[..len].windows(2) {
                    edges.push([w[0], w[1]]);
                }
                edge_list.push(Edge {
                    path,
                    len: len as u8,
                    normals: 0,
                    first: [[0.0; 3]; 2],
                    degenerate: false,
                });
                (edge_list.len() - 1) as u32
            });
            let e = &mut edge_list[i as usize];
            if let Some(n) = e.first.get_mut(e.normals as usize) {
                *n = normal;
            }
            e.normals += 1;
            e.degenerate |= normal == [0.0; 3];
        }
    }
    drop(edge_index);

    let cos_limit = FEATURE_ANGLE_DEG.to_radians().cos();
    let mut feature_edges = Vec::new();
    for e in &edge_list {
        let [n0, n1] = e.first;
        let feature = e.normals != 2 || e.degenerate || dot(n0, n1) < cos_limit;
        if feature {
            for w in e.path[..usize::from(e.len)].windows(2) {
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
    use fh_core::inp::{Element, Node};

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
    fn every_vtk_cell_type_of_the_results_has_an_element_type() {
        for (cell, name) in [
            (VTK_TETRA, "C3D4"),
            (VTK_HEXAHEDRON, "C3D8"),
            (VTK_WEDGE, "C3D6"),
            (VTK_QUADRATIC_TETRA, "C3D10"),
            (VTK_QUADRATIC_HEXAHEDRON, "C3D20"),
            (VTK_QUADRATIC_WEDGE, "C3D15"),
            (VTK_TRIANGLE, "S3"),
            (VTK_QUAD, "S4"),
        ] {
            assert_eq!(vtk_element_type(cell), Some(name));
            assert!(shape(name).is_some());
        }
        assert_eq!(vtk_element_type(fh_core::vtu::VTK_LINE), None);
    }

    #[test]
    fn a_grid_of_quad_and_triangle_cells_has_triangles() {
        let grid = fh_core::vtu::Grid {
            points: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [2.0, 0.0, 0.0],
            ],
            cell_types: vec![VTK_QUAD, VTK_TRIANGLE],
            cells: vec![vec![0, 1, 2, 3], vec![1, 4, 2]],
            point_data: vec![],
            cell_data: vec![],
        };
        let m = build_grid(&grid);
        assert!(m.triangle_cell.contains(&0));
        assert!(m.triangle_cell.contains(&1));
    }
}
