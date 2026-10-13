//! Element level checks: symmetry, rigid body modes, constant strain, loads.
//! Provisional: these tests were written after the code. They may be replaced
//! and they fix no design decision.

use fh_core_solver::element::{
    elasticity, evaluate, mean_stress, pressure_load, stiffness, volume, Elastic, ElementState,
    Evaluation, Gradient, Kind, Material, Placement,
};
use fh_core_solver::SolverError;

static ZERO: [f64; 30] = [0.0; 30];

/// The nodes at their start positions.
fn place(c: &[[f64; 3]]) -> Placement<'_> {
    Placement {
        start: c,
        disp: &ZERO[..3 * c.len()],
    }
}

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
            let t = total(&pressure_load(kind, &place(&c), f as u8 + 1, 2.0).unwrap());
            for k in 0..3 {
                assert!((t[k] - 2.0 * n[k]).abs() < 1e-13, "face {} {t:?}", f + 1);
            }
        }
    }
    assert!(pressure_load(Kind::C3D8, &place(&c), 7, 1.0).is_err());
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
        let t = total(&pressure_load(Kind::C3D10, &place(&c), face, 3.0).unwrap());
        for k in 0..3 {
            assert!(
                (t[k] - 3.0 * area * n[k]).abs() < 1e-13,
                "face {face} {t:?}"
            );
        }
    }
}

#[test]
fn a_curved_tet_face_gets_the_exact_nodal_forces() {
    // Map the unit tet with x -> x + x² in each direction. The quadratic element holds
    // this map exactly. On face 1 (z = 0) the area vector is (0, 0, (1 + 2r)(1 + 2s)).
    let c: Vec<[f64; 3]> = tet([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
        .into_iter()
        .map(|p| p.map(|x| x + x * x))
        .collect();
    let p = 2.0;
    let loads = pressure_load(Kind::C3D10, &place(&c), 1, p).unwrap();
    // The exact force of each face node, from a collapsed Gauss product rule. The rule is
    // exact for this integrand (degree 5 or less in each direction).
    let h = 0.6_f64.sqrt() / 2.0;
    let gauss = [
        (0.5 - h, 5.0 / 18.0),
        (0.5, 8.0 / 18.0),
        (0.5 + h, 5.0 / 18.0),
    ];
    let mut want = [0.0; 6];
    for (a, wa) in gauss {
        for (b, wb) in gauss {
            let (r, s) = (a, (1.0 - a) * b);
            let l = [1.0 - r - s, r, s];
            let n = [
                l[0] * (2.0 * l[0] - 1.0),
                l[1] * (2.0 * l[1] - 1.0),
                l[2] * (2.0 * l[2] - 1.0),
                4.0 * l[0] * l[1],
                4.0 * l[1] * l[2],
                4.0 * l[2] * l[0],
            ];
            let w = wa * wb * (1.0 - a) * p * (1.0 + 2.0 * r) * (1.0 + 2.0 * s);
            for (f, ni) in want.iter_mut().zip(n) {
                *f += w * ni;
            }
        }
    }
    // The closed form for corner node 0 is -2/45 for each unit of pressure.
    assert!((want[0] + p * 2.0 / 45.0).abs() < 1e-14, "{}", want[0]);
    let mut bad = Vec::new();
    for (node, w) in [0, 1, 2, 4, 5, 6].into_iter().zip(want) {
        let got = total(
            &loads
                .iter()
                .filter(|(n, _)| *n == node)
                .copied()
                .collect::<Vec<_>>(),
        );
        if (got[2] - w).abs() > 1e-13 || got[0].abs() > 1e-13 || got[1].abs() > 1e-13 {
            bad.push(format!("node {node}: {got:?}, exact z {w}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_tiny_strain_keeps_its_stress_and_its_force() {
    // A normal strain far below the machine epsilon with an exact stress of 1.
    let c = unit_hex();
    let u = field(&c, |x| [1e-17 * x[0], 0.0, 0.0]);
    let s = mean_stress(Kind::C3D8, &c, &elasticity(1e17, 0.0), &u).unwrap();
    let mut bad = Vec::new();
    if (s[0] - 1.0).abs() > 1e-12 {
        bad.push(format!("stress {s:?}, exact xx 1"));
    }
    // For an ordinary modulus and a tiny field, the internal force is K u.
    let d = elasticity(1000.0, 0.3);
    for kind in KINDS {
        let c = geometry(kind);
        let u = field(&c, |x| {
            [x[0] * x[1], x[1] * x[2] - x[0], x[2] * x[0] + x[1]].map(|v| 1e-14 * v)
        });
        let ku = mat_vec(&stiffness(kind, &c, &d).unwrap(), &u);
        let mut out = Evaluation::default();
        let place = Placement {
            start: &c,
            disp: &u,
        };
        evaluate(
            kind,
            &place,
            &Elastic { d },
            &ElementState::default(),
            &mut out,
            false,
        )
        .unwrap();
        let size = ku.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let err = ku
            .iter()
            .zip(&out.fint)
            .fold(0.0_f64, |m, (a, b)| m.max((a - b).abs()));
        if err > 1e-10 * size {
            bad.push(format!("{kind:?}: |f - K u| = {err:e}, |K u| = {size:e}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_bad_pressure_input_is_an_error_not_a_panic() {
    let c = unit_hex();
    let t = tet([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let (zeros, eleven) = ([0.0; 30], [[0.0; 3]; 11]);
    let at = |start, disp| Placement { start, disp };
    let cases = [
        ("face 0", Kind::C3D8, at(&c, &zeros[..24]), 0),
        ("face 0", Kind::C3D10, at(&t, &zeros), 0),
        ("no displacements", Kind::C3D8, at(&c, &[]), 1),
        (
            "too few displacements",
            Kind::C3D10,
            at(&t, &zeros[..29]),
            1,
        ),
        ("2 nodes", Kind::C3D8, at(&c[..2], &zeros[..6]), 1),
        ("11 nodes", Kind::C3D10, at(&eleven, &zeros), 1),
    ];
    let mut bad = Vec::new();
    for (what, kind, place, face) in cases {
        match std::panic::catch_unwind(|| pressure_load(kind, &place, face, 1.0)) {
            Ok(Ok(_)) => bad.push(format!("{what}: accepted")),
            Ok(Err(_)) => {}
            Err(_) => bad.push(format!("{what}: panic")),
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// A material whose stress is not finite.
struct NotFinite;

impl Material for NotFinite {
    fn update(
        &self,
        _g: &Gradient,
        _old: &[f64],
        _new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        Ok(([f64::NAN; 6], elasticity(1.0, 0.3)))
    }
}

#[test]
fn a_result_that_is_not_finite_is_an_error() {
    let d = elasticity(1.0, 0.3);
    let mut bad = Vec::new();
    // Finite coordinates whose Jacobian determinant overflows.
    let big: Vec<_> = unit_hex().iter().map(|p| p.map(|x| x * 1e110)).collect();
    if let Ok(v) = volume(Kind::C3D8, &big) {
        bad.push(format!("volume of a huge element: {v}"));
    }
    if stiffness(Kind::C3D8, &big, &d).is_ok() {
        bad.push("stiffness of a huge element".into());
    }
    // A finite element and a finite material whose stiffness overflows.
    let small: Vec<_> = unit_hex().iter().map(|p| p.map(|x| x * 1e-100)).collect();
    if stiffness(Kind::C3D8, &small, &elasticity(1e300, 0.3)).is_ok() {
        bad.push("stiffness that overflows".into());
    }
    for kind in KINDS {
        let c = geometry(kind);
        let mut nan_c = c.clone();
        nan_c[1][2] = f64::NAN;
        if volume(kind, &nan_c).is_ok() {
            bad.push(format!("{kind:?}: volume with a NaN coordinate"));
        }
        for v in [f64::NAN, f64::INFINITY] {
            let mut u = vec![0.0; 3 * c.len()];
            u[4] = v;
            let place = Placement {
                start: &c,
                disp: &u,
            };
            let mut out = Evaluation::default();
            let state = ElementState::default();
            if evaluate(kind, &place, &Elastic { d }, &state, &mut out, true).is_ok() {
                bad.push(format!("{kind:?}: displacement {v}"));
            }
        }
        let mut out = Evaluation::default();
        let state = ElementState::default();
        if evaluate(kind, &place(&c), &NotFinite, &state, &mut out, true).is_ok() {
            bad.push(format!("{kind:?}: a material stress that is not finite"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// A material that gets stiffer with the strain: stress = strain³ + 1e-12 strain,
/// with its exact tangent. Newton on the mode amplitudes converges slowly near 0.
struct Cubic;

impl Material for Cubic {
    fn is_linear(&self) -> bool {
        false
    }

    fn update(
        &self,
        g: &Gradient,
        _old: &[f64],
        _new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        let e = g.small_strain();
        let mut d = [[0.0; 6]; 6];
        for (i, row) in d.iter_mut().enumerate() {
            row[i] = 3.0 * e[i] * e[i] + 1e-12;
        }
        Ok((e.map(|x| x * x * x + 1e-12 * x), d))
    }
}

#[test]
fn incompatible_modes_that_do_not_converge_are_an_error() {
    let c = unit_hex();
    let state = ElementState {
        internal: [1.0; 9],
        ..Default::default()
    };
    let mut out = Evaluation::default();
    match evaluate(Kind::C3D8I, &place(&c), &Cubic, &state, &mut out, true) {
        Err(e) => assert!(e.to_string().contains("incompatible modes"), "{e}"),
        Ok(()) => {
            // A converged result is a fixed point: a second call does not move the modes.
            let again = ElementState {
                internal: out.internal,
                ..state
            };
            let mut next = Evaluation::default();
            evaluate(Kind::C3D8I, &place(&c), &Cubic, &again, &mut next, false).unwrap();
            let moved = out
                .internal
                .iter()
                .zip(&next.internal)
                .fold(0.0_f64, |m, (a, b)| m.max((a - b).abs()));
            assert!(
                moved <= 1e-9,
                "accepted modes {:?} that move by {moved:e}",
                out.internal
            );
        }
    }
}
