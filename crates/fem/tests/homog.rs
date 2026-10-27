//! The homogenized material of a periodic unit cell: the exact laminate, Levin's exact relation
//! of the expansion of 2 phases, a square array of cylinders against Rayleigh's formula on the
//! periodic tetrahedral mesh, and the refusal of a cell that is not periodic.

// The checks index the tensors as the formulas do.
#![allow(clippy::needless_range_loop)]

use fh_fem::grid::{Part, Spacing};
use fh_fem::hex::Orthotropic;
use fh_fem::homog::{engineering, invert6, solve, Effective, Events, Material, Model};
use fh_fem::prism::{Footprint, Outline};

struct Quiet;

impl Events for Quiet {
    fn progress(&mut self, _: &str) {}
}

fn block(name: &str, material: usize, priority: i64, lo: [f64; 3], hi: [f64; 3]) -> Part {
    Part {
        name: name.into(),
        material,
        priority,
        lo,
        hi,
        footprint: None,
    }
}

fn material(name: &str, k: f64, e: f64, nu: f64, alpha: f64) -> Material {
    Material {
        name: name.into(),
        conductivity: Some([k; 3]),
        elastic: Some(Orthotropic::isotropic(e, nu)),
        expansion: [alpha; 3],
        density: Some(1.0),
        specific_heat: Some(1.0),
    }
}

fn spacing(max_cell: f64) -> Spacing {
    Spacing {
        max_cell,
        max_growth: 1.3,
        min_cells: 2,
    }
}

fn run(model: &Model) -> Effective {
    solve(model, &mut Quiet).unwrap()
}

fn close(a: f64, b: f64, rel: f64) {
    assert!(
        (a - b).abs() <= rel * b.abs().max(1e-300),
        "{a} against {b}"
    );
}

/// The isotropic stiffness (6 x 6, engineering shear strains).
fn isotropic(e: f64, nu: f64) -> [[f64; 6]; 6] {
    let l = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let g = e / (2.0 * (1.0 + nu));
    let mut c = [[0.0; 6]; 6];
    for i in 0..3 {
        for j in 0..3 {
            c[i][j] = l + if i == j { 2.0 * g } else { 0.0 };
        }
        c[i + 3][i + 3] = g;
    }
    c
}

/// 2 layers along z: the exact laminate. The conductivity is the arithmetic mean along the
/// layers and the harmonic mean across them. The stiffness is the exact laminate of the layer
/// theory (each in-plane strain and each stress across the layers is the same in the 2 layers).
#[test]
fn a_laminate_has_the_exact_bounds_of_each_direction() {
    let (ka, kb) = (1.0, 20.0);
    let (ea, eb, na, nb) = (3.0, 70.0, 0.35, 0.22);
    let (aa, ab) = (60e-6, 20e-6);
    let f = 0.3;
    let model = Model {
        parts: vec![
            block("a", 0, 0, [0.0; 3], [1.0, 1.0, f]),
            block("b", 1, 0, [0.0, 0.0, f], [1.0, 1.0, 1.0]),
        ],
        materials: vec![material("a", ka, ea, na, aa), material("b", kb, eb, nb, ab)],
        spacing: spacing(0.5),
        axis: 2,
    };
    let r = run(&model);
    let k = r.conductivity.unwrap();
    let voigt = f * ka + (1.0 - f) * kb;
    let reuss = 1.0 / (f / ka + (1.0 - f) / kb);
    close(k[0][0], voigt, 1e-9);
    close(k[1][1], voigt, 1e-9);
    close(k[2][2], reuss, 1e-9);
    assert!(k[0][1].abs() < 1e-9 && k[0][2].abs() < 1e-9);
    // The layer theory: the means <.> over the 2 layers.
    let (ca, cb) = (isotropic(ea, na), isotropic(eb, nb));
    let mean = |g: &dyn Fn(&[[f64; 6]; 6]) -> f64| f * g(&ca) + (1.0 - f) * g(&cb);
    let s33 = mean(&|c| 1.0 / c[2][2]);
    let c33 = 1.0 / s33;
    let c13 = mean(&|c| c[0][2] / c[2][2]) / s33;
    let c11 = mean(&|c| c[0][0] - c[0][2] * c[0][2] / c[2][2])
        + mean(&|c| c[0][2] / c[2][2]).powi(2) / s33;
    let c12 = mean(&|c| c[0][1] - c[0][2] * c[1][2] / c[2][2])
        + mean(&|c| c[0][2] / c[2][2]).powi(2) / s33;
    let c44 = 1.0 / mean(&|c| 1.0 / c[3][3]);
    let c66 = mean(&|c| c[5][5]);
    let c = r.stiffness.unwrap();
    close(c[2][2], c33, 1e-8);
    close(c[0][2], c13, 1e-8);
    close(c[0][0], c11, 1e-8);
    close(c[0][1], c12, 1e-8);
    close(c[3][3], c44, 1e-8);
    close(c[4][4], c44, 1e-8);
    close(c[5][5], c66, 1e-8);
    levin(&r, [(ca, aa), (cb, ab)]);
    close(r.fraction[0], f, 1e-12);
    close(r.density.unwrap(), 1.0, 1e-12);
}

/// Levin's exact relation of 2 phases: alpha* = alpha_1 + (S* - S_1) (S_2 - S_1)^-1 (alpha_2 -
/// alpha_1), for isotropic phases with each bulk and shear modulus different.
fn levin(r: &Effective, phases: [([[f64; 6]; 6], f64); 2]) {
    let s = invert6(&r.stiffness.unwrap()).unwrap();
    let s1 = invert6(&phases[0].0).unwrap();
    let s2 = invert6(&phases[1].0).unwrap();
    let d: [[f64; 6]; 6] = std::array::from_fn(|i| std::array::from_fn(|j| s2[i][j] - s1[i][j]));
    let di = invert6(&d).unwrap();
    let a1 = [phases[0].1, phases[0].1, phases[0].1, 0.0, 0.0, 0.0];
    let da: [f64; 6] = std::array::from_fn(|i| {
        if i < 3 {
            phases[1].1 - phases[0].1
        } else {
            0.0
        }
    });
    let x: [f64; 6] = std::array::from_fn(|i| (0..6).map(|j| di[i][j] * da[j]).sum());
    let alpha = r.expansion.unwrap();
    for i in 0..6 {
        let want = a1[i] + (0..6).map(|j| (s[i][j] - s1[i][j]) * x[j]).sum::<f64>();
        assert!(
            (alpha[i] - want).abs() <= 1e-6 * phases[0].1.abs(),
            "alpha {i}: {} against {want}",
            alpha[i]
        );
    }
}

/// A square fibre along z in a matrix: Levin's relation holds for any cell of 2 phases, and the
/// stiffness along the fibre is the mean when the 2 Poisson ratios are equal.
#[test]
fn a_fibre_cell_keeps_the_exact_relations() {
    let (em, ef, nu) = (3.5, 230.0, 0.3);
    let model = Model {
        parts: vec![
            block("matrix", 0, 0, [0.0; 3], [1.0, 1.0, 0.5]),
            block("fibre", 1, 1, [0.2, 0.2, 0.0], [0.8, 0.8, 0.5]),
        ],
        materials: vec![
            material("epoxy", 0.2, em, nu, 60e-6),
            material("carbon", 10.0, ef, nu, -0.5e-6),
        ],
        spacing: spacing(0.1),
        axis: 2,
    };
    let r = run(&model);
    let f = 0.36;
    close(r.fraction[1], f, 1e-12);
    let c = r.stiffness.unwrap();
    let o = engineering(&c).unwrap();
    close(o.e[2], f * ef + (1.0 - f) * em, 1e-6);
    close(o.e[0], o.e[1], 1e-9);
    close(
        r.conductivity.unwrap()[2][2],
        f * 10.0 + (1.0 - f) * 0.2,
        1e-9,
    );
    levin(
        &r,
        [(isotropic(em, nu), 60e-6), (isotropic(ef, nu), -0.5e-6)],
    );
}

/// The transverse conductivity of a square array of cylinders, on the periodic tetrahedral
/// mesh, against the formula of Rayleigh with the lattice sums of Perrins, McKenzie and
/// McPhedran (1979), and inside the Hashin-Shtrikman bounds.
#[test]
fn a_square_array_of_cylinders_matches_rayleigh() {
    let (km, kf, f) = (1.0, 10.0, 0.4);
    let radius = (f / std::f64::consts::PI).sqrt();
    let circle = Part {
        name: "fibre".into(),
        material: 1,
        priority: 1,
        lo: [0.0, 0.0, 0.0],
        hi: [0.0, 0.0, 0.1],
        footprint: Some(Footprint {
            outline: Outline::Circle {
                centre: [0.5, 0.5],
                radius,
            },
            holes: Vec::new(),
        }),
    };
    let heat_only = |name: &str, k: f64| Material {
        name: name.into(),
        conductivity: Some([k; 3]),
        elastic: None,
        expansion: [0.0; 3],
        density: None,
        specific_heat: None,
    };
    let model = Model {
        parts: vec![block("matrix", 0, 0, [0.0; 3], [1.0, 1.0, 0.1]), circle],
        materials: vec![heat_only("m", km), heat_only("f", kf)],
        spacing: Spacing {
            max_cell: 0.02,
            max_growth: 1.2,
            min_cells: 1,
        },
        axis: 2,
    };
    let r = run(&model);
    let k = r.conductivity.unwrap();
    let rho = (kf - km) / (kf + km);
    let rayleigh = km
        * (1.0
            + 2.0 * f * rho
                / (1.0
                    - f * rho
                    - 0.305827 * rho * rho * f.powi(4) / (1.0 - 1.402958 * rho * rho * f.powi(8))
                    - 0.013362 * rho * rho * f.powi(8)));
    let lower = km + f / (1.0 / (kf - km) + (1.0 - f) / (2.0 * km));
    let upper = kf + (1.0 - f) / (1.0 / (km - kf) + f / (2.0 * kf));
    for d in 0..2 {
        assert!(k[d][d] > lower && k[d][d] < upper, "{}", k[d][d]);
        close(k[d][d], rayleigh, 1e-3);
    }
    // The polygon of the circle has less area than the circle.
    close(r.fraction[1], f, 2e-3);
    close(k[2][2], km + r.fraction[1] * (kf - km), 1e-9);
}

#[test]
fn a_cell_whose_opposite_sides_do_not_match_is_refused() {
    let model = Model {
        parts: vec![
            block("a", 0, 0, [0.0; 3], [1.0, 1.0, 1.0]),
            block("b", 0, 0, [1.0, 0.0, 0.0], [2.0, 0.5, 1.0]),
        ],
        materials: vec![material("a", 1.0, 1.0, 0.3, 0.0)],
        spacing: spacing(0.5),
        axis: 2,
    };
    let e = solve(&model, &mut Quiet).unwrap_err();
    assert!(e.contains("not periodic"), "{e}");
}
