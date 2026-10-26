//! One transient probe for surface picks and field extremes. Values come from the field,
//! never from the contour colours. Node probes do not interpolate.
use crate::{
    camera,
    mesh::RenderMesh,
    results::{Location, ScalarField},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    /// Zero-based source index, not a solver node label.
    pub index: usize,
    pub value: f64,
    pub at: Option<[f32; 3]>,
    pub drawn_at: Option<[f32; 3]>,
    pub label: &'static str,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|k| a[k] - b[k])
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| a[k] * b[k]).sum()
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn center(tri: [u32; 3], positions: &[[f32; 3]]) -> [f32; 3] {
    std::array::from_fn(|k| tri.iter().map(|&i| positions[i as usize][k] / 3.0).sum())
}

/// The actual minimum or maximum item, even when the item has no surface representation.
pub fn extreme(
    mesh: &RenderMesh,
    positions: &[[f32; 3]],
    field: &ScalarField,
    max: bool,
) -> Option<Probe> {
    let index = field
        .values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .reduce(|a, b| {
            if if max { b.1 > a.1 } else { b.1 < a.1 } {
                b
            } else {
                a
            }
        })?
        .0;
    item(
        mesh,
        positions,
        field,
        index,
        if max { "Max" } else { "Min" },
    )
}

/// Select an item by its source index for keyboard access.
pub fn at_index(
    mesh: &RenderMesh,
    positions: &[[f32; 3]],
    field: &ScalarField,
    index: usize,
) -> Option<Probe> {
    item(mesh, positions, field, index, "Probe")
}

fn item(
    mesh: &RenderMesh,
    positions: &[[f32; 3]],
    field: &ScalarField,
    index: usize,
    label: &'static str,
) -> Option<Probe> {
    let value = *field.values.get(index).filter(|v| v.is_finite())?;
    let (at, drawn_at) = match field.location {
        Location::Point => {
            let i = mesh.source_node.iter().position(|&n| n as usize == index);
            (i.map(|i| mesh.positions[i]), i.map(|i| positions[i]))
        }
        Location::Cell => {
            let tri = mesh
                .triangle_cell
                .iter()
                .position(|&c| c as usize == index)
                .map(|i| mesh.triangles[i]);
            (
                tri.map(|t| center(t, &mesh.positions)),
                tri.map(|t| center(t, positions)),
            )
        }
    };
    Some(Probe {
        index,
        value,
        at,
        drawn_at,
        label,
    })
}

/// Cast a ray through the current deformed surface. The closest triangle wins, even when
/// its field has no value. For node data, choose its closest vertex to the surface hit.
pub fn pick(
    mesh: &RenderMesh,
    positions: &[[f32; 3]],
    field: &ScalarField,
    origin: [f32; 3],
    direction: [f32; 3],
    matrix: &camera::Mat4,
) -> Option<Probe> {
    let mut nearest = f32::INFINITY;
    let mut hit = None;
    for (i, tri) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = tri.map(|n| positions[n as usize]);
        let (e1, e2) = (sub(b, a), sub(c, a));
        let h = cross(direction, e2);
        let det = dot(e1, h);
        if det.abs() <= f32::EPSILON * dot(e1, e1).sqrt() * dot(e2, e2).sqrt() {
            continue;
        }
        let s = sub(origin, a);
        let u = dot(s, h) / det;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = cross(s, e1);
        let v = dot(direction, q) / det;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let distance = dot(e2, q) / det;
        if distance < 0.0 || distance >= nearest {
            continue;
        }
        let at = std::array::from_fn(|k| origin[k] + distance * direction[k]);
        let clip = camera::transform(matrix, at);
        if clip[3] <= 0.0 || clip[2] < 0.0 || clip[2] > clip[3] {
            continue;
        }
        nearest = distance;
        hit = Some((i, at));
    }
    let (i, at) = hit?;
    match field.location {
        Location::Point => {
            let n = mesh.triangles[i].iter().copied().min_by(|&a, &b| {
                let da = sub(positions[a as usize], at);
                let db = sub(positions[b as usize], at);
                dot(da, da).total_cmp(&dot(db, db))
            })?;
            item(
                mesh,
                positions,
                field,
                mesh.source_node[n as usize] as usize,
                "Probe",
            )
        }
        Location::Cell => {
            let mut probe = item(
                mesh,
                positions,
                field,
                mesh.triangle_cell[i] as usize,
                "Probe",
            )?;
            // Perspective ray intersection gives barycentric weights in world space.
            let [a, b, c] = mesh.triangles[i].map(|n| positions[n as usize]);
            let (e1, e2, p) = (sub(b, a), sub(c, a), sub(at, a));
            let (aa, ab, bb, pa, pb) = (
                dot(e1, e1),
                dot(e1, e2),
                dot(e2, e2),
                dot(p, e1),
                dot(p, e2),
            );
            let den = aa * bb - ab * ab;
            let (u, v) = ((bb * pa - ab * pb) / den, (aa * pb - ab * pa) / den);
            let [a, b, c] = mesh.triangles[i].map(|n| mesh.positions[n as usize]);
            probe.at = Some(std::array::from_fn(|k| {
                a[k] + u * (b[k] - a[k]) + v * (c[k] - a[k])
            }));
            probe.drawn_at = Some(at);
            Some(probe)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (RenderMesh, ScalarField, camera::Mat4) {
        let mesh = RenderMesh {
            positions: vec![
                [-1., -1., 0.2],
                [1., -1., 0.2],
                [0., 1., 0.2],
                [-1., -1., 0.8],
                [1., -1., 0.8],
                [0., 1., 0.8],
            ],
            source_node: vec![2, 0, 1, 3, 4, 5],
            triangles: vec![[3, 4, 5], [0, 1, 2]],
            triangle_cell: vec![1, 0],
            ..Default::default()
        };
        let field = ScalarField {
            name: "NT".into(),
            quantity: crate::results::Quantity::Other,
            location: Location::Point,
            component: false,
            values: vec![10., 20., 30., 40., 50., 60., -5.],
        };
        let identity =
            std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }));
        (mesh, field, identity)
    }
    #[test]
    fn a_pick_uses_the_visible_triangle_and_original_node_value() {
        let (mesh, field, m) = sample();
        let p = pick(
            &mesh,
            &mesh.positions,
            &field,
            [0., 0.8, 0.],
            [0., 0., 1.],
            &m,
        )
        .unwrap();
        assert_eq!((p.index, p.value), (1, 20.));
        assert_eq!(p.at, Some([0., 1., 0.2]));
        assert!(pick(
            &mesh,
            &mesh.positions,
            &field,
            [2., 2., 0.],
            [0., 0., 1.],
            &m
        )
        .is_none());
        let min = extreme(&mesh, &mesh.positions, &field, false).unwrap();
        assert_eq!((min.index, min.value, min.at), (6, -5., None));
    }
    #[test]
    fn cell_identity_follows_the_deformed_front_surface_and_missing_values_do_not_pick_through() {
        let (mesh, mut field, m) = sample();
        field.location = Location::Cell;
        field.values = vec![100., 200.];
        let mut moved = mesh.positions.clone();
        for p in &mut moved[..3] {
            p[2] = 0.9;
        }
        let p = pick(&mesh, &moved, &field, [0., 0., 0.], [0., 0., 1.], &m).unwrap();
        assert_eq!((p.index, p.value), (1, 200.));
        assert!((p.at.unwrap()[2] - 0.8).abs() < 1e-6);
        field.values[1] = f64::NAN;
        assert!(pick(&mesh, &moved, &field, [0., 0., 0.], [0., 0., 1.], &m).is_none());
    }
}
