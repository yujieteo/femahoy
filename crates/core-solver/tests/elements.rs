//! Element level checks: symmetry, rigid body modes, constant strain, loads.

use fh_core_solver::element::{elasticity, mean_stress, pressure_load, stiffness, volume, Kind};

const HEX: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [2.0, 0.1, 0.0],
    [2.2, 1.5, 0.2],
    [0.1, 1.2, 0.0],
    [0.0, 0.1, 1.1],
    [1.9, 0.0, 1.0],
    [2.0, 1.4, 1.3],
    [0.2, 1.3, 1.2],
];

fn unit_hex() -> Vec<[f64; 3]> {
    let s = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ];
    s.to_vec()
}

fn tet(corners: [[f64; 3]; 4]) -> Vec<[f64; 3]> {
    let mid = |a: usize, b: usize| {
        let mut m = [0.0; 3];
        for k in 0..3 {
            m[k] = 0.5 * (corners[a][k] + corners[b][k]);
        }
        m
    };
    let mut v = corners.to_vec();
    for (a, b) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)] {
        v.push(mid(a, b));
    }
    v
}

fn skew_tet() -> Vec<[f64; 3]> {
    let mut t = tet([
        [0.0, 0.0, 0.0],
        [1.3, 0.1, 0.0],
        [0.2, 0.9, 0.1],
        [0.1, 0.2, 1.1],
    ]);
    // Move the midside nodes off the edge centres: the edges curve.
    t[4][2] += 0.03;
    t[7][0] -= 0.02;
    t[9][1] += 0.02;
    t
}

const KINDS: [Kind; 3] = [Kind::C3D8, Kind::C3D8I, Kind::C3D10];

fn geometry(kind: Kind) -> Vec<[f64; 3]> {
    match kind {
        Kind::C3D10 => skew_tet(),
        _ => HEX.to_vec(),
    }
}

fn field(coords: &[[f64; 3]], f: impl Fn([f64; 3]) -> [f64; 3]) -> Vec<f64> {
    coords.iter().flat_map(|&x| f(x)).collect()
}

fn mat_vec(k: &[f64], u: &[f64]) -> Vec<f64> {
    let n = u.len();
    (0..n)
        .map(|i| (0..n).map(|j| k[i * n + j] * u[j]).sum())
        .collect()
}

#[test]
fn stiffness_is_symmetric() {
    let d = elasticity(200.0, 0.3);
    for kind in KINDS {
        let k = stiffness(kind, &geometry(kind), &d).unwrap();
        let n = 3 * kind.node_count();
        let scale = k.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        for i in 0..n {
            for j in 0..n {
                assert!(
                    (k[i * n + j] - k[j * n + i]).abs() < 1e-12 * scale,
                    "{kind:?}"
                );
            }
        }
    }
}

#[test]
fn rigid_body_motion_gives_no_force() {
    let d = elasticity(200.0, 0.3);
    for kind in KINDS {
        let c = geometry(kind);
        let k = stiffness(kind, &c, &d).unwrap();
        let scale = k.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let modes: [Vec<f64>; 6] = [
            field(&c, |_| [1.0, 0.0, 0.0]),
            field(&c, |_| [0.0, 1.0, 0.0]),
            field(&c, |_| [0.0, 0.0, 1.0]),
            field(&c, |x| [0.0, -x[2], x[1]]),
            field(&c, |x| [x[2], 0.0, -x[0]]),
            field(&c, |x| [-x[1], x[0], 0.0]),
        ];
        for m in &modes {
            let f = mat_vec(&k, m);
            let worst = f.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
            assert!(worst < 1e-11 * scale, "{kind:?}: {worst:e}");
        }
    }
}

#[test]
fn linear_field_gives_exact_constant_stress() {
    let (e, nu) = (210.0, 0.25);
    let d = elasticity(e, nu);
    // Gradient of u: exx=1e-3, eyy=-2e-3, ezz=5e-4, gxy=4e-4, gyz=-3e-4, gxz=2e-4.
    let g = [
        [1.0e-3, 2.0e-4, 1.0e-4],
        [2.0e-4, -2.0e-3, -1.5e-4],
        [1.0e-4, -1.5e-4, 5.0e-4],
    ];
    let eps = [1.0e-3, -2.0e-3, 5.0e-4, 4.0e-4, -3.0e-4, 2.0e-4];
    for kind in KINDS {
        let c = geometry(kind);
        let u = field(&c, |x| {
            let mut r = [0.0; 3];
            for i in 0..3 {
                r[i] = (0..3).map(|j| g[i][j] * x[j]).sum();
            }
            r
        });
        let s = mean_stress(kind, &c, &d, &u).unwrap();
        for i in 0..6 {
            let want: f64 = (0..6).map(|j| d[i][j] * eps[j]).sum();
            assert!(
                (s[i] - want).abs() < 1e-10,
                "{kind:?} comp {i}: {} vs {want}",
                s[i]
            );
        }
    }
}

#[test]
fn volumes_match_geometry() {
    assert!((volume(Kind::C3D8, &unit_hex()).unwrap() - 1.0).abs() < 1e-14);
    assert!((volume(Kind::C3D8I, &unit_hex()).unwrap() - 1.0).abs() < 1e-14);
    let t = tet([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    assert!((volume(Kind::C3D10, &t).unwrap() - 1.0 / 6.0).abs() < 1e-14);
}

#[test]
fn inverted_element_is_rejected() {
    let mut c = unit_hex();
    c.swap(0, 4);
    c.swap(1, 5);
    c.swap(2, 6);
    c.swap(3, 7);
    let err = stiffness(Kind::C3D8, &c, &elasticity(1.0, 0.3)).unwrap_err();
    assert!(err.to_string().contains("non-positive Jacobian"), "{err}");
}

#[test]
fn wrong_node_count_is_rejected() {
    let err = stiffness(Kind::C3D10, &unit_hex(), &elasticity(1.0, 0.3)).unwrap_err();
    assert!(err.to_string().contains("needs 10 nodes"), "{err}");
}

fn total(loads: &[(usize, [f64; 3])]) -> [f64; 3] {
    let mut t = [0.0; 3];
    for (_, f) in loads {
        for k in 0..3 {
            t[k] += f[k];
        }
    }
    t
}

#[test]
fn hex_pressure_pushes_into_the_body() {
    let c = unit_hex();
    // Inward normals of faces 1 to 6 for the unit cube.
    let inward = [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        [0.0, 1.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0],
    ];
    for kind in [Kind::C3D8, Kind::C3D8I] {
        for (f, n) in inward.iter().enumerate() {
            let t = total(&pressure_load(kind, &c, f as u8 + 1, 2.0).unwrap());
            for k in 0..3 {
                assert!((t[k] - 2.0 * n[k]).abs() < 1e-13, "face {} {t:?}", f + 1);
            }
        }
    }
    assert!(pressure_load(Kind::C3D8, &c, 7, 1.0).is_err());
}

#[test]
fn tet_pressure_pushes_into_the_body() {
    let c = tet([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let s3 = 3.0_f64.sqrt();
    let cases: [(u8, [f64; 3], f64); 4] = [
        (1, [0.0, 0.0, 1.0], 0.5),
        (2, [0.0, 1.0, 0.0], 0.5),
        (3, [-1.0 / s3, -1.0 / s3, -1.0 / s3], s3 / 2.0),
        (4, [1.0, 0.0, 0.0], 0.5),
    ];
    for (face, n, area) in cases {
        let t = total(&pressure_load(Kind::C3D10, &c, face, 3.0).unwrap());
        for k in 0..3 {
            assert!(
                (t[k] - 3.0 * area * n[k]).abs() < 1e-13,
                "face {face} {t:?}"
            );
        }
    }
}
