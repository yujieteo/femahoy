//! Mesh checks: counts, bounding box, volume and the signed-Jacobian gate.
//! Decision D-19, see DESIGN.md: every mesh passes a signed-Jacobian gate.

use std::collections::HashMap;

use fh_inp::Model;

/// The result of the checks on one mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshStats {
    pub nodes: usize,
    /// Element count for each element type, sorted by type name.
    pub types: Vec<(String, usize)>,
    pub bbox_min: [f64; 3],
    pub bbox_max: [f64; 3],
    /// Volume from the corner nodes. Midside nodes do not count.
    pub volume: f64,
    /// The smallest scaled Jacobian at any element corner. It is 1 for a cube corner.
    pub min_scaled_jacobian: f64,
    /// The number of elements with a corner at or below 0.
    pub inverted: usize,
    /// The ids of the first elements with such a corner.
    pub inverted_ids: Vec<u32>,
}

type P = [f64; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn det(a: P, b: P, c: P) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

fn norm(a: P) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// The determinant of the three edges at a corner, and the scaled value (det / product of lengths).
fn corner(p: &[P], at: usize, n: [usize; 3]) -> (f64, f64) {
    let (a, b, c) = (
        sub(p[n[0]], p[at]),
        sub(p[n[1]], p[at]),
        sub(p[n[2]], p[at]),
    );
    let d = det(a, b, c);
    let len = norm(a) * norm(b) * norm(c);
    (d, if len > 0.0 { d / len } else { 0.0 })
}

/// For each hex corner: the three neighbours that make a right-handed triple.
const HEX_CORNERS: [(usize, [usize; 3]); 8] = [
    (0, [1, 3, 4]),
    (1, [2, 0, 5]),
    (2, [3, 1, 6]),
    (3, [0, 2, 7]),
    (4, [5, 0, 7]),
    (5, [6, 1, 4]),
    (6, [7, 2, 5]),
    (7, [4, 3, 6]),
];

/// The 6 tets of a hex around the diagonal 0-6.
const HEX_TETS: [[usize; 4]; 6] = [
    [0, 1, 2, 6],
    [0, 2, 3, 6],
    [0, 3, 7, 6],
    [0, 7, 4, 6],
    [0, 4, 5, 6],
    [0, 5, 1, 6],
];

/// The volume and the smallest scaled Jacobian of one element. `None` for an unknown type.
fn element_metrics(kind: &str, p: &[P]) -> Option<(f64, f64)> {
    match kind {
        "C3D10" => {
            let (d, s) = corner(p, 0, [1, 2, 3]);
            let min = [
                s,
                corner(p, 1, [0, 3, 2]).1,
                corner(p, 2, [0, 1, 3]).1,
                corner(p, 3, [0, 2, 1]).1,
            ]
            .into_iter()
            .fold(f64::INFINITY, f64::min);
            Some((d / 6.0, min))
        }
        "C3D8I" | "C3D8" => {
            let min = HEX_CORNERS
                .iter()
                .map(|(at, n)| corner(p, *at, *n).1)
                .fold(f64::INFINITY, f64::min);
            let volume = HEX_TETS
                .iter()
                .map(|t| corner(p, t[0], [t[1], t[2], t[3]]).0 / 6.0)
                .sum();
            Some((volume, min))
        }
        _ => None,
    }
}

/// Check a mesh. An element with a missing node or an unknown type is an error.
pub fn mesh_stats(model: &Model) -> Result<MeshStats, String> {
    let coords: HashMap<u32, P> = model.nodes.iter().map(|n| (n.id, n.xyz)).collect();
    let mut bbox_min = [f64::INFINITY; 3];
    let mut bbox_max = [f64::NEG_INFINITY; 3];
    for n in &model.nodes {
        for i in 0..3 {
            bbox_min[i] = bbox_min[i].min(n.xyz[i]);
            bbox_max[i] = bbox_max[i].max(n.xyz[i]);
        }
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let (mut volume, mut min_sj) = (0.0, f64::INFINITY);
    let (mut inverted, mut inverted_ids) = (0, Vec::new());
    for e in &model.elements {
        let p: Vec<P> = e
            .nodes
            .iter()
            .map(|id| {
                coords
                    .get(id)
                    .copied()
                    .ok_or_else(|| format!("element {} uses the missing node {id}", e.id))
            })
            .collect::<Result<_, _>>()?;
        let kind = e.element_type.as_str();
        let (v, sj) = element_metrics(kind, &p)
            .ok_or_else(|| format!("element {} has the unchecked type {kind}", e.id))?;
        *counts.entry(kind).or_default() += 1;
        volume += v;
        min_sj = min_sj.min(sj);
        if sj.is_nan() || sj <= 0.0 {
            inverted += 1;
            if inverted_ids.len() < 5 {
                inverted_ids.push(e.id);
            }
        }
    }
    let mut types: Vec<(String, usize)> =
        counts.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    types.sort();
    Ok(MeshStats {
        nodes: model.nodes.len(),
        types,
        bbox_min,
        bbox_max,
        volume,
        min_scaled_jacobian: min_sj,
        inverted,
        inverted_ids,
    })
}
