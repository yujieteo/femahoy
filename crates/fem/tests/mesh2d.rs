//! The quality triangle mesh: the angle bound, conformity, the constrained Delaunay property,
//! the area of each region, and the end of the refinement at small input angles.

use std::collections::HashMap;

use fh_fem::mesh2d::{mesh, Mesh2, Size, MIN_ANGLE, OUTSIDE};
use fh_fem::predicates::incircle;

/// The points and the closed loop of segments of each polygon.
fn loops(polygons: &[Vec<[f64; 2]>]) -> (Vec<[f64; 2]>, Vec<[u32; 2]>) {
    let mut points = Vec::new();
    let mut segments = Vec::new();
    for poly in polygons {
        let base = points.len() as u32;
        let n = poly.len() as u32;
        points.extend_from_slice(poly);
        for k in 0..n {
            segments.push([base + k, base + (k + 1) % n]);
        }
    }
    (points, segments)
}

fn circle(c: [f64; 2], r: f64, n: usize) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for k in 0..poly.len() {
        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

fn area(poly: &[[f64; 2]]) -> f64 {
    (0..poly.len())
        .map(|k| {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        * 0.5
}

fn tri_area(m: &Mesh2, t: usize) -> f64 {
    let [a, b, c] = m.triangles[t].map(|v| m.points[v as usize]);
    0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
}

/// The smallest angle of each triangle, in degrees.
fn min_angle(m: &Mesh2, t: usize) -> f64 {
    let p = m.triangles[t].map(|v| m.points[v as usize]);
    (0..3)
        .map(|i| {
            let (a, b, c) = (p[i], p[(i + 1) % 3], p[(i + 2) % 3]);
            let u = [b[0] - a[0], b[1] - a[1]];
            let w = [c[0] - a[0], c[1] - a[1]];
            let cos = (u[0] * w[0] + u[1] * w[1]) / (u[0].hypot(u[1]) * w[0].hypot(w[1]));
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .fold(180.0, f64::min)
}

/// The checks of every mesh: each triangle is counterclockwise; each edge has 1 or 2 triangles,
/// and an edge with 1 triangle is on a segment (conformity: no hanging point); an edge with 2
/// triangles that is not on a segment has the empty circle property.
fn check(m: &Mesh2) {
    let mut edges: HashMap<(u32, u32), Vec<(usize, u32)>> = HashMap::new();
    for (t, tri) in m.triangles.iter().enumerate() {
        assert!(tri_area(m, t) > 0.0, "triangle {t} is not counterclockwise");
        for i in 0..3 {
            let (a, b) = (tri[(i + 1) % 3], tri[(i + 2) % 3]);
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((t, tri[i]));
        }
    }
    for (e, list) in &edges {
        assert!(list.len() <= 2, "edge {e:?} has {} triangles", list.len());
        if list.len() == 1 {
            assert!(
                m.segments.contains_key(e),
                "edge {e:?} is on the boundary, not on a segment"
            );
        } else if !m.segments.contains_key(e) {
            let [a, b, c] = m.triangles[list[0].0].map(|v| m.points[v as usize]);
            let d = m.points[list[1].1 as usize];
            assert!(incircle(a, b, c, d) <= 0.0, "edge {e:?} is not Delaunay");
        }
    }
}

fn size(max_size: f64) -> Size {
    Size {
        max_size,
        max_growth: 1.3,
    }
}

#[test]
fn a_square_has_the_angle_bound_and_its_area() {
    let sq = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let (p, s) = loops(std::slice::from_ref(&sq));
    let m = mesh(&p, &s, size(0.1), &|x| {
        if inside(&sq, x) {
            0
        } else {
            OUTSIDE
        }
    })
    .unwrap();
    check(&m);
    let total: f64 = (0..m.triangles.len()).map(|t| tri_area(&m, t)).sum();
    assert!((total - 1.0).abs() < 1e-12, "{total}");
    let worst = (0..m.triangles.len())
        .map(|t| min_angle(&m, t))
        .fold(180.0, f64::min);
    assert!(worst >= MIN_ANGLE, "{worst}");
    // The size field: no edge is much longer than max_size.
    for tri in &m.triangles {
        for i in 0..3 {
            let (a, b) = (
                m.points[tri[i] as usize],
                m.points[tri[(i + 1) % 3] as usize],
            );
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 0.1 * 1.5);
        }
    }
}

/// A plate with a hole: the mesh is graded from the small segments of the hole to max_size.
#[test]
fn a_plate_with_a_hole_is_graded_and_has_the_angle_bound() {
    let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
    let hole = circle([1.0, 1.0], 0.3, 48);
    let (p, s) = loops(&[outer.clone(), hole.clone()]);
    let m = mesh(&p, &s, size(0.5), &|x| {
        if inside(&outer, x) && !inside(&hole, x) {
            7
        } else {
            OUTSIDE
        }
    })
    .unwrap();
    check(&m);
    let total: f64 = (0..m.triangles.len()).map(|t| tri_area(&m, t)).sum();
    assert!((total - (8.0 - area(&hole))).abs() < 1e-12, "{total}");
    assert!(m.label.iter().all(|&l| l == 7));
    let worst = (0..m.triangles.len())
        .map(|t| min_angle(&m, t))
        .fold(180.0, f64::min);
    assert!(worst >= MIN_ANGLE, "{worst}");
    // Small triangles at the hole, large ones at the far end.
    let mean_edge = |pred: &dyn Fn([f64; 2]) -> bool| {
        let (mut sum, mut n) = (0.0, 0);
        for (t, tri) in m.triangles.iter().enumerate() {
            let c = tri.map(|v| m.points[v as usize]);
            let centre = [
                (c[0][0] + c[1][0] + c[2][0]) / 3.0,
                (c[0][1] + c[1][1] + c[2][1]) / 3.0,
            ];
            if pred(centre) {
                sum += tri_area(&m, t).sqrt();
                n += 1;
            }
        }
        sum / n as f64
    };
    let near = mean_edge(&|c| (c[0] - 1.0).hypot(c[1] - 1.0) < 0.45);
    let far = mean_edge(&|c| c[0] > 3.5);
    assert!(far > 3.0 * near, "near {near} far {far}");
}

/// 2 overlapping squares and a triangle that crosses both: the segments cross, so the clean
/// splits them. Each region has the label of the shapes that hold it, and the areas add up.
#[test]
fn crossing_outlines_give_regions_with_the_right_areas() {
    let a = vec![[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
    let b = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
    // An edge of c runs along the edge y = 0 of a, and its slanted edges cross both squares.
    let c = vec![[0.5, 0.0], [1.5, 0.0], [1.0, 2.5]];
    let (p, s) = loops(&[a.clone(), b.clone(), c.clone()]);
    let m = mesh(&p, &s, size(0.25), &|x| {
        let bits = inside(&a, x) as u32 | (inside(&b, x) as u32) << 1 | (inside(&c, x) as u32) << 2;
        if bits == 0 {
            OUTSIDE
        } else {
            bits
        }
    })
    .unwrap();
    check(&m);
    let mut by_label: HashMap<u32, f64> = HashMap::new();
    for t in 0..m.triangles.len() {
        *by_label.entry(m.label[t]).or_default() += tri_area(&m, t);
    }
    let total: f64 = by_label.values().sum();
    // The union: 4 + 4 - 1 (the overlap), and the part of c outside both squares (the tip above
    // y = 2 that is left of x = 1): c is above y = 2 for a height of 0.5 with a base of 0.2.
    let tip_left = 0.5 * 0.2 * 0.5 * 0.5;
    assert!((total - (7.0 + tip_left)).abs() < 1e-9, "{total}");
    let in_a: f64 = by_label
        .iter()
        .filter(|(l, _)| **l & 1 != 0)
        .map(|(_, v)| v)
        .sum();
    let in_b: f64 = by_label
        .iter()
        .filter(|(l, _)| **l & 2 != 0)
        .map(|(_, v)| v)
        .sum();
    let in_c: f64 = by_label
        .iter()
        .filter(|(l, _)| **l & 4 != 0)
        .map(|(_, v)| v)
        .sum();
    assert!(
        (in_a - 4.0).abs() < 1e-9 && (in_b - 4.0).abs() < 1e-9,
        "{in_a} {in_b}"
    );
    assert!((in_c - area(&c)).abs() < 1e-9, "{in_c}");
    let worst = (0..m.triangles.len())
        .map(|t| min_angle(&m, t))
        .fold(180.0, f64::min);
    // The crossings make input angles below 60 degrees, where the bound does not hold.
    assert!(worst > 5.0, "{worst}");
}

/// A thin wedge with a 5 degree tip: the refinement ends, and only triangles at the tip are
/// below the bound.
#[test]
fn a_small_input_angle_does_not_stop_the_refinement_from_ending() {
    let t = 5f64.to_radians();
    let wedge = vec![[0.0, 0.0], [1.0, 0.0], [t.cos(), t.sin()]];
    let (p, s) = loops(std::slice::from_ref(&wedge));
    let m = mesh(&p, &s, size(0.05), &|x| {
        if inside(&wedge, x) {
            0
        } else {
            OUTSIDE
        }
    })
    .unwrap();
    check(&m);
    let total: f64 = (0..m.triangles.len()).map(|t| tri_area(&m, t)).sum();
    assert!((total - area(&wedge)).abs() < 1e-12);
    for t in 0..m.triangles.len() {
        if min_angle(&m, t) < MIN_ANGLE {
            let c = m.triangles[t].map(|v| m.points[v as usize]);
            let r = c.iter().map(|p| p[0].hypot(p[1])).fold(0.0, f64::max);
            assert!(r < 0.5, "a skinny triangle far from the tip, at r = {r}");
        }
    }
}
