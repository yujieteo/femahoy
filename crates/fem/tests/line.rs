//! The transmission line solver against exact answers: the coaxial line, the skin effect of a
//! round wire (Bessel functions), and the low frequency limit of the resistance.

use fh_fem::grid::Part;
use fh_fem::line::{self, Conduction, Events, Material, Model, EPS0, MU0};
use fh_fem::prism::{Footprint, Outline};

struct Quiet;
impl Events for Quiet {
    fn progress(&mut self, _: &str) {}
}

fn material(name: &str, permittivity: f64, conduction: Conduction) -> Material {
    Material {
        name: name.into(),
        permittivity,
        loss_tangent: 0.0,
        permeability: 1.0,
        conduction,
        roughness: 0.0,
    }
}

fn circle(name: &str, material: usize, priority: i64, r: f64, hole: Option<f64>) -> Part {
    Part {
        name: name.into(),
        material,
        priority,
        lo: [0.0; 3],
        hi: [0.0, 0.0, 1.0],
        footprint: Some(Footprint {
            outline: Outline::Circle {
                centre: [0.0, 0.0],
                radius: r,
            },
            holes: hole
                .map(|h| Outline::Circle {
                    centre: [0.0, 0.0],
                    radius: h,
                })
                .into_iter()
                .collect(),
        }),
    }
}

fn rect(name: &str, material: usize, priority: i64, lo: [f64; 2], hi: [f64; 2]) -> Part {
    Part {
        name: name.into(),
        material,
        priority,
        lo: [lo[0], lo[1], 0.0],
        hi: [hi[0], hi[1], 1.0],
        footprint: None,
    }
}

fn model(parts: Vec<Part>, materials: Vec<Material>, max_cell: f64) -> Model {
    Model {
        parts,
        materials,
        signals: vec![0],
        references: vec![],
        electric_walls: [false; 4],
        frequencies: vec![1e9],
        max_cell,
        max_growth: 1.3,
        max_points: 400_000,
        length: None,
        reference_impedance: 50.0,
    }
}

/// A coaxial line of perfect conductors with a dielectric of eps_r = 2.25: C = 2 pi eps /
/// ln(b/a), L = mu0 / (2 pi) ln(b/a), Z0 = sqrt(L / C) and eps_eff = eps_r. The polygons of the
/// circles have segments of max_cell, so C and L are within about (max_cell / a)^2 / 10.
#[test]
fn a_coaxial_line_of_perfect_conductors_has_the_exact_c_l_and_z0() {
    let (a, b) = (0.5e-3, 1.75e-3);
    let mut m = model(
        vec![
            circle("inner", 1, 1, a, None),
            circle("fill", 0, 0, b, None),
            circle("outer", 1, 0, b + 0.2e-3, Some(b)),
        ],
        vec![
            material("ptfe", 2.25, Conduction::None),
            material("pec", 1.0, Conduction::Perfect),
        ],
        0.02e-3,
    );
    m.references = vec![2];
    m.length = Some(0.1);
    let s = line::solve(&m, &mut Quiet).unwrap();
    let p = &s.params[0];
    let ln = (b / a).ln();
    let c = 2.0 * std::f64::consts::PI * EPS0 * 2.25 / ln;
    let l = MU0 / (2.0 * std::f64::consts::PI) * ln;
    let rel = |x: f64, want: f64| (x - want).abs() / want;
    assert!(rel(p.c[0], c) < 2e-3, "C {} against {c}", p.c[0]);
    assert!(rel(p.l[0], l) < 2e-3, "L {} against {l}", p.l[0]);
    assert!(rel(p.z0.unwrap().re, (l / c).sqrt()) < 2e-3);
    assert!(rel(p.modes[0].eps_eff, 2.25) < 1e-3);
    assert_eq!((p.r[0], p.g[0]), (0.0, 0.0));
    // A lossless line: |S11|^2 + |S21|^2 = 1.
    let sm = p.s.as_ref().unwrap();
    let power = sm.at(0, 0).abs().powi(2) + sm.at(1, 0).abs().powi(2);
    assert!((power - 1.0).abs() < 1e-9, "{power}");
}

/// A round copper wire of radius 1 mm in a square box with electric walls. The resistance is
/// the real part of k J0(ka) / (2 pi a sigma J1(ka)), k = (1 - j) / delta (the box adds only an
/// inductance). The values are from mpmath at 30 digits.
#[test]
fn the_resistance_of_a_round_wire_follows_the_bessel_function_answer() {
    let a = 1e-3;
    let mut m = model(
        vec![
            circle("wire", 1, 1, a, None),
            rect("air", 0, 0, [-10.0 * a, -10.0 * a], [10.0 * a, 10.0 * a]),
        ],
        vec![
            material("air", 1.0, Conduction::None),
            material("copper", 1.0, Conduction::Finite(5.8e7)),
        ],
        1e-3,
    );
    m.electric_walls = [true; 4];
    m.frequencies = vec![1e3, 1e5, 1e6];
    let s = line::solve(&m, &mut Quiet).unwrap();
    for (p, r) in s
        .params
        .iter()
        .zip([5.49409079963e-3, 1.46073104771e-2, 4.29286576531e-2])
    {
        let err = (p.r[0] - r).abs() / r;
        assert!(
            err < 3e-3,
            "{} Hz: R {} against {r}, {err:.2e}",
            p.frequency,
            p.r[0]
        );
    }
    // The current crowds at the surface at 1 MHz (a / delta = 15).
    let (mut centre, mut edge) = (0.0, 0.0f64);
    for (e, t) in s.triangles.iter().enumerate() {
        let c = t.iter().fold([0.0; 2], |acc, &v| {
            let q = s.points[v as usize];
            [acc[0] + q[0] / 3.0, acc[1] + q[1] / 3.0]
        });
        let r = c[0].hypot(c[1]);
        if r < 0.2 * a {
            centre = f64::max(centre, s.current[0][e]);
        } else if r < a {
            edge = edge.max(s.current[0][e]);
        }
    }
    assert!(edge > 1e3 * centre, "edge {edge}, centre {centre}");
}

/// At a low frequency the resistance of a rectangular bar with a reference bar beside it is the
/// sum of their DC resistances 1 / (sigma A), and R, L, C and G are symmetric matrices for 2
/// signals.
#[test]
fn the_low_frequency_resistance_is_the_dc_resistance_and_the_matrices_are_symmetric() {
    let sigma = 3.5e7;
    let mut m = model(
        vec![
            rect("a", 1, 1, [-3e-3, 0.0], [-1e-3, 1e-3]),
            rect("b", 1, 1, [1e-3, 0.0], [3e-3, 0.5e-3]),
            rect("ground", 1, 1, [-1e-3, 3e-3], [1e-3, 4e-3]),
            rect("air", 0, 0, [-8e-3, -5e-3], [8e-3, 8e-3]),
        ],
        vec![
            {
                let mut d = material("dielectric", 3.0, Conduction::None);
                d.loss_tangent = 0.02;
                d
            },
            material("aluminium", 1.0, Conduction::Finite(sigma)),
        ],
        0.5e-3,
    );
    m.signals = vec![0, 1];
    m.references = vec![2];
    m.frequencies = vec![1.0, 1e9];
    m.max_points = 1_000_000;
    m.length = Some(0.05);
    let s = line::solve(&m, &mut Quiet).unwrap();
    let dc = &s.params[0];
    let ground = 1.0 / (sigma * 2e-6);
    for (i, area) in [2e-6, 1e-6].into_iter().enumerate() {
        let want = 1.0 / (sigma * area) + ground;
        let r = dc.r[i * 2 + i];
        assert!((r - want).abs() / want < 1e-6, "R{i}{i} {r} against {want}");
    }
    assert!((dc.r[1] - ground).abs() / ground < 1e-6);
    for p in &s.params {
        for m in [&p.r, &p.l, &p.g, &p.c] {
            assert!((m[1] - m[2]).abs() <= 1e-6 * m[0].abs(), "{m:?}");
        }
        // G = omega C tan(delta) in a single dielectric.
        let w = 2.0 * std::f64::consts::PI * p.frequency;
        assert!((p.g[0] - w * p.c[0] * 0.02).abs() < 1e-9 * w * p.c[0]);
        let sm = p.s.as_ref().unwrap();
        assert!((sm.at(0, 2) - sm.at(2, 0)).abs() < 1e-9);
    }
    // The skin effect raises R and lowers L at 1 GHz.
    let hf = &s.params[1];
    assert!(hf.r[0] > 10.0 * dc.r[0] && hf.l[0] < dc.l[0]);
}

/// A line out of the class is refused with a message that tells why.
#[test]
fn a_wrong_line_is_refused() {
    let parts = vec![
        rect("strip", 1, 1, [-1e-3, 0.0], [1e-3, 0.1e-3]),
        rect("air", 0, 0, [-5e-3, -1e-3], [5e-3, 5e-3]),
    ];
    let materials = vec![
        material("air", 1.0, Conduction::None),
        material("copper", 1.0, Conduction::Finite(5.8e7)),
    ];
    let m = model(parts.clone(), materials.clone(), 0.5e-3);
    let e = line::solve(&m, &mut Quiet).unwrap_err();
    assert!(e.contains("needs a reference"), "{e}");
    let mut m = model(parts, materials, 0.5e-3);
    m.electric_walls = [false, false, true, false];
    m.frequencies = vec![100e9];
    m.max_points = 10_000;
    let e = line::solve(&m, &mut Quiet).unwrap_err();
    assert!(e.contains("skin depth"), "{e}");
    m.frequencies = vec![0.0];
    assert!(line::solve(&m, &mut Quiet)
        .unwrap_err()
        .contains("above 0 Hz"));
}

/// With 1 conductor the loss is R, so the Hammerstad factor multiplies R and leaves L.
#[test]
fn the_roughness_multiplies_the_resistance_of_a_single_conductor_by_the_hammerstad_factor() {
    let a = 1e-3;
    let wire = |roughness: f64| {
        let mut copper = material("copper", 1.0, Conduction::Finite(5.8e7));
        copper.roughness = roughness;
        let mut m = model(
            vec![
                circle("wire", 1, 1, a, None),
                rect("air", 0, 0, [-5.0 * a, -5.0 * a], [5.0 * a, 5.0 * a]),
            ],
            vec![material("air", 1.0, Conduction::None), copper],
            1e-3,
        );
        m.electric_walls = [true; 4];
        m.frequencies = vec![1e5];
        line::solve(&m, &mut Quiet).unwrap().params.remove(0)
    };
    let (smooth, rough) = (wire(0.0), wire(0.3e-3));
    let delta = line::skin_depth(1e5, 1.0, 5.8e7);
    let k = line::roughness_factor(0.3e-3, delta);
    assert!(k > 1.5 && k < 2.0, "{k}");
    assert!(
        (rough.r[0] / smooth.r[0] - k).abs() < 1e-9,
        "{}",
        rough.r[0] / smooth.r[0]
    );
    assert!((rough.l[0] - smooth.l[0]).abs() < 1e-12 * smooth.l[0]);
}
