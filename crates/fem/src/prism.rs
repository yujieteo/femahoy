//! The 2.5D tetrahedral mesh of parts that are prisms: a footprint (a polygon or a circle, with
//! holes) across the extrusion axis, between 2 planes on that axis. A box is the prism of a
//! rectangle.
//!
//! 1. The outlines of all footprints go into 1 quality triangle mesh (`mesh2d`), so the
//!    triangles of 2 parts that touch share their edges. A circle is a polygon with its points on
//!    the circle.
//! 2. The planes of the extrusion axis are the graded grid planes of the box grid (`grid`): a
//!    plane at each end of each part, and graded planes between them.
//! 3. Each triangle and each layer between 2 planes is a prism. It takes the part that holds the
//!    centre, the part with the highest priority where parts overlap, as a cell of the box grid
//!    does. A prism in no part is void.
//! 4. Each prism is cut into 3 tetrahedra. The diagonal of each quadrilateral face goes through
//!    the corner with the smallest global index, so 2 prisms that share a face cut it the same
//!    way, and the mesh is conforming.
//!
//! Sources: J. Dompierre, P. Labbé, M.-G. Vallet and R. Camarero, How to subdivide pyramids,
//! prisms and hexahedra into tetrahedra, Proc. 8th International Meshing Roundtable (1999)
//! 195-204 (the rule of the smallest index and its table of rotations); P. J. Frey and P.-L.
//! George, Mesh Generation, 2nd ed., Wiley 2008, section 8.2 (extruded meshes).

use std::cell::RefCell;
use std::collections::HashMap;

use crate::grid::{axis_planes, tol, Part, Spacing};
use crate::mesh2d::{self, Size, OUTSIDE};

/// A closed outline in the plane across the extrusion axis.
#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    /// The corners, in either direction.
    Polygon(Vec<[f64; 2]>),
    Circle {
        centre: [f64; 2],
        radius: f64,
    },
}

/// The shape of a part across the extrusion axis: an outline with holes.
#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
    pub outline: Outline,
    pub holes: Vec<Outline>,
}

/// The fewest segments of a circle. Reason: a circle with fewer segments is not round: the
/// polygon of 16 has 2.6 % less area than the circle, and the polygon of 4 has 36 % less.
const MIN_CIRCLE_SEGMENTS: usize = 16;

/// The coordinates of a point in the plane across the axis `axis`: the next 2 axes in the order
/// x, y, z, x (for the axis x: y and z; for y: z and x; for z: x and y).
pub fn across(axis: usize, p: [f64; 3]) -> [f64; 2] {
    [p[(axis + 1) % 3], p[(axis + 2) % 3]]
}

fn point(axis: usize, uv: [f64; 2], w: f64) -> [f64; 3] {
    let mut p = [0.0; 3];
    p[axis] = w;
    p[(axis + 1) % 3] = uv[0];
    p[(axis + 2) % 3] = uv[1];
    p
}

impl Outline {
    /// The corners of the outline, a circle as a polygon with segments of at most `max_cell`.
    pub fn corners(&self, max_cell: f64) -> Vec<[f64; 2]> {
        match self {
            Outline::Polygon(p) => p.clone(),
            Outline::Circle { centre, radius } => {
                let n = ((2.0 * std::f64::consts::PI * radius / max_cell).ceil() as usize)
                    .max(MIN_CIRCLE_SEGMENTS);
                (0..n)
                    .map(|k| {
                        let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
                        [centre[0] + radius * a.cos(), centre[1] + radius * a.sin()]
                    })
                    .collect()
            }
        }
    }
}

/// True when the point `p` is inside the polygon `corners` (even-odd rule).
fn inside(corners: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for k in 0..corners.len() {
        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

/// A face of the mesh with a tetrahedron on 1 side only.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryFace {
    /// The 3 points, counterclockwise from outside.
    pub nodes: [u32; 3],
    pub tet: u32,
    /// The circles (indices into `TetMesh::circles`) whose outline holds the face.
    pub circles: Vec<u32>,
}

/// The tetrahedral mesh of the parts.
#[derive(Debug, Clone, PartialEq)]
pub struct TetMesh {
    pub points: Vec<[f64; 3]>,
    /// The 4 points of each tetrahedron, with a positive volume.
    pub tets: Vec<[u32; 4]>,
    /// The part of each tetrahedron.
    pub part: Vec<u32>,
    pub faces: Vec<BoundaryFace>,
    /// The centre and the radius of each circle outline of the parts.
    pub circles: Vec<([f64; 2], f64)>,
    /// The number of triangles of the 2D mesh and of planes of the extrusion axis.
    pub triangles: usize,
    pub planes: usize,
}

/// The rotations of a prism (bottom 0 1 2, top 3 4 5, 3 above 0) that bring each corner to
/// corner 0 (Dompierre et al., table 2).
const ROTATE: [[usize; 6]; 6] = [
    [0, 1, 2, 3, 4, 5],
    [1, 2, 0, 4, 5, 3],
    [2, 0, 1, 5, 3, 4],
    [3, 5, 4, 0, 2, 1],
    [4, 3, 5, 1, 0, 2],
    [5, 4, 3, 2, 1, 0],
];

/// The 3 tetrahedra of a prism with the global indices `g` of its 6 corners (bottom 0 1 2, top
/// 3 4 5, 3 above 0), as local corners.
pub fn split_prism(g: [u64; 6]) -> [[usize; 4]; 3] {
    let m = (0..6).min_by_key(|&k| g[k]).expect("6 corners");
    let r = ROTATE[m];
    let gr = |k: usize| g[r[k]];
    let t = if gr(1).min(gr(5)) < gr(2).min(gr(4)) {
        [[0, 1, 2, 5], [0, 1, 5, 4], [0, 4, 5, 3]]
    } else {
        [[0, 1, 2, 4], [0, 4, 2, 5], [0, 4, 5, 3]]
    };
    t.map(|tet| tet.map(|k| r[k]))
}

fn volume(p: [[f64; 3]; 4]) -> f64 {
    let d = |k: usize| [p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]];
    let (a, b, c) = (d(1), d(2), d(3));
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]))
        / 6.0
}

/// An outline and its holes, as polygons.
type Ring2 = (Vec<[f64; 2]>, Vec<Vec<[f64; 2]>>);

/// The mesh of `parts` (each with a footprint, or a box) extruded along `axis`.
pub fn mesh(parts: &[Part], axis: usize, spacing: Spacing) -> Result<TetMesh, String> {
    if parts.is_empty() {
        return Err("the model has no part".into());
    }
    if !(spacing.max_cell > 0.0 && spacing.max_cell.is_finite()) {
        return Err("max_cell must be a number above 0".into());
    }
    // The outlines of each part (outline first, then the holes), as polygons.
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut segments: Vec<[u32; 2]> = Vec::new();
    // The circle of the outline of each segment, if it is a circle.
    let mut segment_circle: Vec<u32> = Vec::new();
    let mut circles: Vec<([f64; 2], f64)> = Vec::new();
    // The outline and the holes of each part, as polygons.
    let mut polygons: Vec<Ring2> = Vec::new();
    for p in parts {
        if !(p.lo[axis].is_finite() && p.hi[axis].is_finite() && p.lo[axis] < p.hi[axis]) {
            return Err(format!(
                "part {}: its {} range must be 2 finite numbers, the first smaller",
                p.name,
                ["x", "y", "z"][axis]
            ));
        }
        let footprint = match &p.footprint {
            Some(f) => f.clone(),
            None => {
                let (lo, hi) = (across(axis, p.lo), across(axis, p.hi));
                if !(lo[0] < hi[0] && lo[1] < hi[1]) {
                    return Err(format!(
                        "part {}: each range must have the first number smaller",
                        p.name
                    ));
                }
                Footprint {
                    outline: Outline::Polygon(vec![lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]]),
                    holes: Vec::new(),
                }
            }
        };
        let mut rings = Vec::new();
        for outline in std::iter::once(&footprint.outline).chain(&footprint.holes) {
            let corners = match outline {
                Outline::Circle { centre, radius } => {
                    if !(radius.is_finite()
                        && *radius > 0.0
                        && centre.iter().all(|c| c.is_finite()))
                    {
                        return Err(format!(
                            "part {}: a circle needs a finite centre and a radius above 0",
                            p.name
                        ));
                    }
                    outline.corners(spacing.max_cell)
                }
                Outline::Polygon(c) => {
                    if c.len() < 3 || c.iter().flatten().any(|v| !v.is_finite()) {
                        return Err(format!(
                            "part {}: a polygon needs 3 or more corners with finite coordinates",
                            p.name
                        ));
                    }
                    c.clone()
                }
            };
            let area: f64 = (0..corners.len())
                .map(|k| {
                    let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum();
            if area == 0.0 {
                return Err(format!("part {}: an outline has no area", p.name));
            }
            let circle = match outline {
                Outline::Circle { centre, radius } => {
                    circles.push((*centre, *radius));
                    circles.len() as u32 - 1
                }
                Outline::Polygon(_) => u32::MAX,
            };
            let base = points.len() as u32;
            let n = corners.len() as u32;
            points.extend_from_slice(&corners);
            for k in 0..n {
                segments.push([base + k, base + (k + 1) % n]);
                segment_circle.push(circle);
            }
            rings.push(corners);
        }
        let outline = rings.remove(0);
        polygons.push((outline, rings));
    }
    // The parts that hold a point of the plane, highest priority first, interned as labels.
    let covers: RefCell<Vec<Vec<u32>>> = RefCell::new(Vec::new());
    let classify = |x: [f64; 2]| -> u32 {
        let mut list: Vec<u32> = (0..parts.len() as u32)
            .filter(|&k| {
                let (outline, holes) = &polygons[k as usize];
                inside(outline, x) && !holes.iter().any(|h| inside(h, x))
            })
            .collect();
        if list.is_empty() {
            return OUTSIDE;
        }
        // Highest priority first; the later part first at the same priority.
        list.sort_by(|&a, &b| {
            parts[b as usize]
                .priority
                .cmp(&parts[a as usize].priority)
                .then(b.cmp(&a))
        });
        let mut c = covers.borrow_mut();
        match c.iter().position(|l| *l == list) {
            Some(k) => k as u32,
            None => {
                c.push(list);
                c.len() as u32 - 1
            }
        }
    };
    let flat = mesh2d::mesh(
        &points,
        &segments,
        Size {
            max_size: spacing.max_cell,
            max_growth: spacing.max_growth,
        },
        &classify,
    )?;
    let covers = covers.into_inner();
    let planes = axis_planes(parts, axis, spacing, &[])?;
    let t = tol(&planes);
    // The part of each layer of each label.
    let layers = planes.len() - 1;
    let mut part_of = vec![u32::MAX; covers.len() * layers];
    for (l, list) in covers.iter().enumerate() {
        for k in 0..layers {
            let (z0, z1) = (planes[k], planes[k + 1]);
            let holds = |p: &Part| p.lo[axis] <= z0 + t && p.hi[axis] >= z1 - t;
            let Some(&first) = list.iter().find(|&&p| holds(&parts[p as usize])) else {
                continue;
            };
            let a = &parts[first as usize];
            if let Some(&other) = list.iter().find(|&&p| {
                p != first && {
                    let b = &parts[p as usize];
                    holds(b) && b.priority == a.priority && b.material != a.material
                }
            }) {
                return Err(format!(
                    "parts {} and {} overlap with the same priority and other materials: give \
                     the part that must hold the overlap a higher priority",
                    parts[other as usize].name, a.name
                ));
            }
            part_of[l * layers + k] = first;
        }
    }
    let nv = flat.points.len();
    let slots = nv
        .checked_mul(planes.len())
        .filter(|&s| s <= u32::MAX as usize)
        .ok_or("the mesh has too many points: use a larger max_cell")?;
    let mut number = vec![u32::MAX; slots];
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut tets: Vec<[u32; 4]> = Vec::new();
    let mut part: Vec<u32> = Vec::new();
    for (tri, &label) in flat.triangles.iter().zip(&flat.label) {
        for k in 0..layers {
            let p = part_of[label as usize * layers + k];
            if p == u32::MAX {
                continue;
            }
            let g: [u64; 6] = std::array::from_fn(|c| {
                let plane = k + c / 3;
                (plane * nv + tri[c % 3] as usize) as u64
            });
            let mut node = [0u32; 6];
            for c in 0..6 {
                let slot = &mut number[g[c] as usize];
                if *slot == u32::MAX {
                    *slot = pts.len() as u32;
                    let uv = flat.points[tri[c % 3] as usize];
                    pts.push(point(axis, uv, planes[k + c / 3]));
                }
                node[c] = *slot;
            }
            for local in split_prism(g) {
                let mut tet = local.map(|c| node[c]);
                if volume(tet.map(|v| pts[v as usize])) < 0.0 {
                    tet.swap(2, 3);
                }
                tets.push(tet);
                part.push(p);
            }
        }
    }
    if tets.is_empty() {
        return Err("the parts hold no volume".into());
    }
    // The faces with 1 tetrahedron.
    let mut seen: HashMap<[u32; 3], (u32, u8, [u32; 3])> = HashMap::new();
    for (e, tet) in tets.iter().enumerate() {
        // The faces of a tetrahedron with a positive volume, counterclockwise from outside.
        for f in [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]] {
            let nodes = f.map(|k| tet[k]);
            let mut key = nodes;
            key.sort_unstable();
            seen.entry(key)
                .and_modify(|v| v.1 += 1)
                .or_insert((e as u32, 1, nodes));
        }
    }
    // The 2D point of each 3D point.
    let mut flat_of = vec![0u32; pts.len()];
    for (slot, &n) in number.iter().enumerate() {
        if n != u32::MAX {
            flat_of[n as usize] = (slot % nv) as u32;
        }
    }
    let mut faces: Vec<BoundaryFace> = seen
        .into_values()
        .filter(|v| v.1 == 1)
        .map(|(tet, _, nodes)| {
            let mut flat_nodes: Vec<u32> = nodes.iter().map(|&n| flat_of[n as usize]).collect();
            flat_nodes.sort_unstable();
            flat_nodes.dedup();
            let circles = if flat_nodes.len() == 2 {
                let mut list: Vec<u32> = flat
                    .segments
                    .get(&(flat_nodes[0], flat_nodes[1]))
                    .map(|s| s.iter().map(|&k| segment_circle[k as usize]).collect())
                    .unwrap_or_default();
                list.retain(|&c| c != u32::MAX);
                list.sort_unstable();
                list.dedup();
                list
            } else {
                Vec::new()
            };
            BoundaryFace {
                nodes,
                tet,
                circles,
            }
        })
        .collect();
    faces.sort_by_key(|f| (f.tet, f.nodes));
    Ok(TetMesh {
        points: pts,
        tets,
        part,
        faces,
        circles,
        triangles: flat.triangles.len(),
        planes: planes.len(),
    })
}
