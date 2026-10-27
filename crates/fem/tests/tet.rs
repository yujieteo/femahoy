//! The 2.5D tetrahedral mesh and heat conduction on it: conformity, the volume and the surface,
//! exact linear fields along each extrusion axis, the thick cylinder wall against its analytic
//! answer, and the energy balance of a transient solve.

use std::collections::HashMap;

use fh_fem::grid::{Part, Spacing};
use fh_fem::heat::{
    solve, solve_within, Events, FaceSet, Frame, Limit, Load, Material, Mesh, Model, Probe, Step,
};
use fh_fem::prism::{mesh, split_prism, Footprint, Outline};

#[derive(Default)]
struct Record {
    mesh: Option<Mesh>,
    frames: Vec<Frame>,
}

impl Events for Record {
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.mesh = Some(mesh.clone());
        Ok(())
    }
    fn progress(&mut self, _: &str) {}
    fn frame(&mut self, frame: &Frame) -> Result<(), String> {
        self.frames.push(frame.clone());
        Ok(())
    }
}

fn shaped(
    name: &str,
    material: usize,
    outline: Outline,
    holes: Vec<Outline>,
    axis: usize,
    range: [f64; 2],
) -> Part {
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    lo[axis] = range[0];
    hi[axis] = range[1];
    Part {
        name: name.into(),
        material,
        priority: 0,
        lo,
        hi,
        footprint: Some(Footprint { outline, holes }),
    }
}

fn spacing(max_cell: f64) -> Spacing {
    Spacing {
        max_cell,
        max_growth: 1.3,
        min_cells: 2,
    }
}

fn faces(name: &str) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: vec![],
        region: None,
        parts: vec![],
        circle: None,
    }
}

fn material(k: f64) -> Material {
    Material {
        name: format!("k{k}"),
        conductivity: [k; 3],
        capacity: 1.0,
    }
}

fn tet_volume(p: [[f64; 3]; 4]) -> f64 {
    let d = |k: usize| [p[k][0] - p[0][0], p[k][1] - p[0][1], p[k][2] - p[0][2]];
    let (a, b, c) = (d(1), d(2), d(3));
    (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
        + a[2] * (b[0] * c[1] - b[1] * c[0]))
        / 6.0
}

/// Each of the 720 orders of the 6 corners gives 3 tetrahedra that fill the prism, and the
/// diagonal of each quadrilateral face goes through its corner with the smallest index.
#[test]
fn each_prism_split_fills_the_prism_and_cuts_each_face_at_its_smallest_corner() {
    let corners = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    let quads = [[0, 1, 4, 3], [1, 2, 5, 4], [2, 0, 3, 5]];
    let mut perm = [0u64, 1, 2, 3, 4, 5];
    let mut count = 0;
    // Heap's algorithm over the global indices.
    fn each(k: usize, a: &mut [u64; 6], f: &mut dyn FnMut(&[u64; 6])) {
        if k == 1 {
            f(a);
            return;
        }
        for i in 0..k {
            each(k - 1, a, f);
            if k.is_multiple_of(2) {
                a.swap(i, k - 1);
            } else {
                a.swap(0, k - 1);
            }
        }
    }
    each(6, &mut perm, &mut |g| {
        count += 1;
        let tets = split_prism(*g);
        let total: f64 = tets
            .iter()
            .map(|t| tet_volume(t.map(|k| corners[k])).abs())
            .sum();
        assert!((total - 0.5).abs() < 1e-12, "{g:?}");
        for q in quads {
            let min = *q.iter().min_by_key(|&&k| g[k]).unwrap();
            let pos = q.iter().position(|&k| k == min).unwrap();
            let opposite = q[(pos + 2) % 4];
            // The diagonal min-opposite is an edge of a tetrahedron.
            assert!(
                tets.iter()
                    .any(|t| t.contains(&min) && t.contains(&opposite)),
                "{g:?} {q:?}"
            );
        }
    });
    assert_eq!(count, 720);
}

/// A plate with a round hole and a polygon on its top: each inner face has 2 tetrahedra, the
/// volume is exact for the polygons, and the boundary faces cover the surface.
#[test]
fn the_extruded_mesh_is_conforming_and_has_the_volume_of_the_parts() {
    let hole = Outline::Circle {
        centre: [1.0, 1.0],
        radius: 0.4,
    };
    let plate = shaped(
        "plate",
        0,
        Outline::Polygon(vec![[0.0, 0.0], [3.0, 0.0], [3.0, 2.0], [0.0, 2.0]]),
        vec![hole.clone()],
        2,
        [0.0, 0.2],
    );
    let lid = shaped(
        "lid",
        1,
        Outline::Polygon(vec![[2.0, 0.5], [2.8, 0.5], [2.4, 1.5]]),
        vec![],
        2,
        [0.2, 0.5],
    );
    let m = mesh(&[plate, lid], 2, spacing(0.2), usize::MAX, "").unwrap();
    let mut volume = [0.0; 2];
    for (t, tet) in m.tets.iter().enumerate() {
        let v = tet_volume(tet.map(|i| m.points[i as usize]));
        assert!(v > 0.0);
        volume[m.part[t] as usize] += v;
    }
    let n = hole.corners(0.2).len() as f64;
    let polygon_hole = 0.5 * n * 0.4f64.powi(2) * (2.0 * std::f64::consts::PI / n).sin();
    assert!(
        (volume[0] - (6.0 - polygon_hole) * 0.2).abs() < 1e-9,
        "{}",
        volume[0]
    );
    assert!((volume[1] - 0.4 * 0.3).abs() < 1e-9, "{}", volume[1]);
    let mut count: HashMap<[u32; 3], u32> = HashMap::new();
    for tet in &m.tets {
        for f in [[1, 2, 3], [0, 3, 2], [0, 1, 3], [0, 2, 1]] {
            let mut key = f.map(|k| tet[k]);
            key.sort_unstable();
            *count.entry(key).or_default() += 1;
        }
    }
    assert!(count.values().all(|&c| c <= 2));
    // The boundary faces: the sum of their outward area vectors is 0 for a closed surface.
    let mut sum = [0.0; 3];
    let mut on_hole = 0.0;
    for f in &m.faces {
        let p = f.nodes.map(|i| m.points[i as usize]);
        let u: [f64; 3] = std::array::from_fn(|d| p[1][d] - p[0][d]);
        let w: [f64; 3] = std::array::from_fn(|d| p[2][d] - p[0][d]);
        let n = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        for d in 0..3 {
            sum[d] += 0.5 * n[d];
        }
        if !f.circles.is_empty() {
            on_hole += 0.5 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        }
    }
    assert!(sum.iter().all(|v| v.abs() < 1e-12), "{sum:?}");
    assert_eq!(count.values().filter(|&&c| c == 1).count(), m.faces.len());
    // The faces on the hole: the perimeter of the polygon times the height.
    let perimeter = n * 2.0 * 0.4 * (std::f64::consts::PI / n).sin();
    assert!((on_hole - perimeter * 0.2).abs() < 1e-9, "{on_hole}");
}

/// A wall of 2 materials with a slanted inner face: the temperature is linear in each material
/// when the heat flows normal to the layers. The extrusion is along x, y or z: the result is
/// the same.
#[test]
fn a_linear_field_is_exact_on_tetrahedra_along_each_axis() {
    for axis in 0..3 {
        // In the plane across the axis: layer a from u = 0 to 1, layer b from u = 1 to 3.
        let a = shaped(
            "a",
            0,
            Outline::Polygon(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
            vec![],
            axis,
            [0.0, 0.5],
        );
        let b = shaped(
            "b",
            1,
            Outline::Polygon(vec![[1.0, 0.0], [3.0, 0.0], [3.0, 1.0], [1.0, 1.0]]),
            vec![],
            axis,
            [0.0, 0.5],
        );
        let side = |u: f64| {
            let mut lo = [-1.0; 3];
            let mut hi = [4.0; 3];
            lo[(axis + 1) % 3] = u;
            hi[(axis + 1) % 3] = u;
            FaceSet {
                region: Some((lo, hi)),
                ..faces(&format!("u{u}"))
            }
        };
        let mut probe_at = [0.0; 3];
        probe_at[axis] = 0.25;
        probe_at[(axis + 1) % 3] = 1.0;
        probe_at[(axis + 2) % 3] = 0.37;
        let model = Model {
            parts: vec![a, b],
            materials: vec![material(1.0), material(4.0)],
            spacing: spacing(0.25),
            axis,
            symmetry: [false; 3],
            faces: vec![side(0.0), side(3.0)],
            loads: vec![
                Load::Temperature {
                    faces: 0,
                    value: 100.0,
                },
                Load::Temperature {
                    faces: 1,
                    value: 0.0,
                },
            ],
            step: Step::Steady,
            probes: vec![Probe {
                name: "interface".into(),
                point: probe_at,
            }],
            method: None,
        };
        let mut r = Record::default();
        let s = solve(&model, &mut r).unwrap();
        // Resistances per area: 1 / 1 + 2 / 4 = 1.5; q = 100 / 1.5.
        let q = 100.0 / 1.5;
        let f = &r.frames[0];
        assert!(
            (f.probes[0] - (100.0 - q)).abs() < 1e-8,
            "axis {axis}: {}",
            f.probes[0]
        );
        assert!(s.imbalance < 1e-9);
        let along = (axis + 1) % 3;
        assert!(
            f.flux.iter().all(|v| (v[along] - q).abs() < 1e-6),
            "axis {axis}"
        );
    }
}

/// The ring r1 < r < r2 with T1 at r1 and T2 at r2: T = T1 + (T2 - T1) ln(r / r1) / ln(r2 / r1).
/// The largest error at the points falls by about 4 when max_cell halves (second order).
#[test]
fn a_thick_cylinder_wall_converges_to_the_analytic_answer() {
    let (r1, r2) = (1.0, 2.0);
    let model = |h: f64| Model {
        parts: vec![shaped(
            "wall",
            0,
            Outline::Circle {
                centre: [0.0, 0.0],
                radius: r2,
            },
            vec![Outline::Circle {
                centre: [0.0, 0.0],
                radius: r1,
            }],
            2,
            [0.0, 0.1],
        )],
        materials: vec![material(1.0)],
        spacing: spacing(h),
        axis: 2,
        symmetry: [false; 3],
        faces: vec![
            FaceSet {
                circle: Some(([0.0, 0.0], r1)),
                ..faces("inner")
            },
            FaceSet {
                circle: Some(([0.0, 0.0], r2)),
                ..faces("outer")
            },
        ],
        loads: vec![
            Load::Temperature {
                faces: 0,
                value: 100.0,
            },
            Load::Temperature {
                faces: 1,
                value: 0.0,
            },
        ],
        step: Step::Steady,
        probes: vec![],
        method: None,
    };
    let err = |h: f64| {
        let mut r = Record::default();
        solve(&model(h), &mut r).unwrap();
        let m = r.mesh.unwrap();
        m.points
            .iter()
            .zip(&r.frames[0].temperature)
            .map(|(p, t)| {
                let r = p[0].hypot(p[1]);
                (t - 100.0 * (1.0 - (r / r1).ln() / (r2 / r1).ln())).abs()
            })
            .fold(0.0, f64::max)
    };
    let (e1, e2) = (err(0.2), err(0.1));
    assert!(e2 < 0.25 && e2 < e1 / 3.0, "{e1} {e2}");
}

/// A transient on tetrahedra: the energy balance holds at each step, and the end is the steady
/// state.
#[test]
fn a_transient_on_tetrahedra_keeps_the_energy_balance() {
    let disk = shaped(
        "disk",
        0,
        Outline::Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        },
        vec![],
        2,
        [0.0, 0.2],
    );
    let model = |step| Model {
        parts: vec![disk.clone()],
        materials: vec![material(1.0)],
        spacing: spacing(0.25),
        axis: 2,
        symmetry: [false; 3],
        faces: vec![FaceSet {
            circle: Some(([0.0, 0.0], 1.0)),
            ..faces("rim")
        }],
        loads: vec![
            Load::Convection {
                faces: 0,
                film: 10.0,
                sink: 20.0,
            },
            Load::Source {
                part: 0,
                value: 50.0,
            },
        ],
        step,
        probes: vec![Probe {
            name: "centre".into(),
            point: [0.0, 0.0, 0.1],
        }],
        method: None,
    };
    let mut steady = Record::default();
    solve(&model(Step::Steady), &mut steady).unwrap();
    let mut tr = Record::default();
    let s = solve(
        &model(Step::Transient {
            end: 20.0,
            dt: 0.5,
            initial: 20.0,
            output_every: 8,
        }),
        &mut tr,
    )
    .unwrap();
    assert!(s.imbalance < 1e-9, "{}", s.imbalance);
    let last = tr.frames.last().unwrap();
    assert!(
        (last.probes[0] - steady.frames[0].probes[0]).abs() < 1e-3,
        "{} {}",
        last.probes[0],
        steady.frames[0].probes[0]
    );
}

/// A box over part of a face keeps the triangles whose centroid is in the box, as the box grid
/// keeps the faces whose centre is in it: the flux on the patch puts in the flux times the area
/// of those triangles.
#[test]
fn a_box_over_part_of_a_face_keeps_the_triangles_whose_centroid_is_in_it() {
    let plate = shaped(
        "plate",
        0,
        Outline::Polygon(vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        vec![],
        2,
        [0.0, 0.2],
    );
    let (lo, hi) = ([-1.0, -1.0, 0.2], [0.37, 2.0, 0.2]);
    let m = mesh(std::slice::from_ref(&plate), 2, spacing(0.25), usize::MAX, "").unwrap();
    let (mut centroid_in, mut corners_in) = (0.0, 0.0);
    for f in &m.faces {
        let p = f.nodes.map(|i| m.points[i as usize]);
        if p.iter().any(|q| (q[2] - 0.2).abs() > 1e-12) {
            continue;
        }
        let area = 0.5
            * ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]))
                .abs();
        if (p[0][0] + p[1][0] + p[2][0]) / 3.0 <= hi[0] {
            centroid_in += area;
        }
        if p.iter().all(|q| q[0] <= hi[0]) {
            corners_in += area;
        }
    }
    assert!(corners_in < centroid_in, "{corners_in} {centroid_in}");
    let model = Model {
        parts: vec![plate],
        materials: vec![material(1.0)],
        spacing: spacing(0.25),
        axis: 2,
        symmetry: [false; 3],
        faces: vec![
            FaceSet {
                region: Some((lo, hi)),
                ..faces("patch")
            },
            FaceSet {
                region: Some(([-1.0, -1.0, 0.0], [2.0, 2.0, 0.0])),
                ..faces("bottom")
            },
        ],
        loads: vec![
            Load::Flux {
                faces: 0,
                value: 1.0,
            },
            Load::Temperature {
                faces: 1,
                value: 0.0,
            },
        ],
        step: Step::Steady,
        probes: vec![],
        method: None,
    };
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    let flux = r.frames[0].energy.flux;
    assert!((flux - centroid_in).abs() < 1e-12, "{flux} {centroid_in}");
}

/// A mesh of tetrahedra with more points than the limit is refused with the limit, the memory
/// and the way to a smaller mesh.
#[test]
fn a_mesh_over_the_tet_limit_is_refused() {
    let disk = shaped(
        "disk",
        0,
        Outline::Circle {
            centre: [0.0, 0.0],
            radius: 1.0,
        },
        vec![],
        2,
        [0.0, 0.2],
    );
    let model = Model {
        parts: vec![disk],
        materials: vec![material(1.0)],
        spacing: spacing(0.25),
        axis: 2,
        symmetry: [false; 3],
        faces: vec![],
        loads: vec![],
        step: Step::Steady,
        probes: vec![],
        method: None,
    };
    let limit = Limit {
        points: 1_000_000,
        tet_points: 10,
        memory: "a small memory",
    };
    let e = solve_within(&model, limit, &mut Record::default()).unwrap_err();
    assert!(
        e.contains("more than 10 points") && e.contains("a small memory") && e.contains("max_cell"),
        "{e}"
    );
}
