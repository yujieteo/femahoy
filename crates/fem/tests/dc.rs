//! DC conduction on the box grid: the exact resistance of a bar, a current terminal, the Joule
//! heat of a bar against the exact temperature, the coupling of a temperature-dependent
//! conductivity, and the refusals of thermal runaway and of a model with no reference.

use fh_fem::dc::{solve, Drive, Events, Kind, Material, Model, Solution, Terminal};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{self, FaceSet, Load};

struct Quiet;

impl Events for Quiet {
    fn progress(&mut self, _: &str) {}
}

fn side(name: &str, s: &str) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: vec![Side::parse(s).unwrap()],
        region: None,
        parts: vec![],
        circle: None,
    }
}

const SPACING: Spacing = Spacing {
    max_cell: 0.01,
    max_growth: 1.3,
    min_cells: 2,
};

/// A bar from x = 0 to `length`, 0.01 x 0.01 in y and z, of conductivity `sigma`, with the
/// terminals `left` at x = 0 and `right` at x = `length`.
fn bar(length: f64, sigma: f64, left: Drive, right: Drive) -> Model {
    Model {
        parts: vec![Part {
            name: "bar".into(),
            material: 0,
            priority: 0,
            lo: [0.0; 3],
            hi: [length, 0.01, 0.01],
            footprint: None,
        }],
        materials: vec![Material {
            name: "metal".into(),
            value: [sigma; 3],
            coefficient: 0.0,
        }],
        spacing: SPACING,
        axis: 2,
        faces: vec![
            side("left", "x-"),
            side("right", "x+"),
            FaceSet {
                name: "all".into(),
                sides: vec![],
                region: None,
                parts: vec![],
                circle: None,
            },
        ],
        terminals: vec![
            Terminal {
                faces: 0,
                drive: left,
            },
            Terminal {
                faces: 1,
                drive: right,
            },
        ],
        kind: Kind::Conduction,
        scale: 1.0,
        heat: None,
        reference: 0.0,
        probes: vec![],
        method: None,
    }
}

/// The steady heat model of the bar with the conductivity `k` and the loads `loads`.
fn heat_of(model: &Model, k: f64, loads: Vec<Load>) -> Box<heat::Model> {
    Box::new(heat::Model {
        parts: model.parts.clone(),
        materials: vec![heat::Material {
            name: "metal".into(),
            conductivity: [k; 3],
            capacity: 0.0,
            conductivity_table: None,
            capacity_table: None,
        }],
        spacing: model.spacing,
        axis: model.axis,
        symmetry: [false; 3],
        faces: model.faces.clone(),
        loads,
        step: heat::Step::Steady,
        probes: vec![],
        method: None,
    })
}

fn run(model: &Model) -> Solution {
    solve(model, &mut Quiet).unwrap()
}

#[test]
fn a_uniform_bar_has_the_exact_resistance_and_current_density() {
    let s = run(&bar(0.3, 2.0, Drive::Voltage(1.0), Drive::Voltage(0.0)));
    let exact = 0.3 / (2.0 * 1e-4);
    let (v, i) = s.terminals[0];
    assert!((v / i / exact - 1.0).abs() < 1e-9, "{}", v / i);
    assert!((s.terminals[1].1 + i).abs() < 1e-12 * i);
    // J = sigma V / L along x, and the Joule power density sigma (V / L)^2.
    let j = 2.0 / 0.3;
    for (f, q) in s.field.iter().zip(&s.joule) {
        assert!((f[0] / j - 1.0).abs() < 1e-9 && f[1].abs() < 1e-9 && f[2].abs() < 1e-9);
        assert!((q / (j * j / 2.0) - 1.0).abs() < 1e-9);
    }
    assert!((s.power() - i).abs() < 1e-12);
}

#[test]
fn a_current_terminal_is_1_potential_with_the_given_current() {
    let s = run(&bar(0.3, 2.0, Drive::Current(0.1), Drive::Voltage(0.0)));
    let (v, i) = s.terminals[0];
    assert!((i - 0.1).abs() < 1e-12, "{i}");
    assert!((v / (0.1 * 1500.0) - 1.0).abs() < 1e-9, "{v}");
    let points = &s.mesh.points;
    for (p, u) in points.iter().zip(&s.potential) {
        if p[0] == 0.0 {
            assert!((u - v).abs() < 1e-12);
        }
    }
}

#[test]
fn the_joule_heat_of_a_bar_gives_the_exact_peak_temperature() {
    // Ends at 20 C, sides adiabatic: T_max = T0 + q L^2 / (8 k) with q = sigma (V / L)^2.
    let (l, sigma, v, k) = (0.2, 1e5, 0.01, 50.0);
    let mut model = bar(l, sigma, Drive::Voltage(v), Drive::Voltage(0.0));
    model.spacing.max_cell = l / 20.0;
    model.heat = Some(heat_of(
        &model,
        k,
        vec![
            Load::Temperature {
                faces: 0,
                value: 20.0,
            },
            Load::Temperature {
                faces: 1,
                value: 20.0,
            },
        ],
    ));
    let s = run(&model);
    let t = s.temperature.unwrap();
    let peak = t.iter().copied().fold(f64::MIN, f64::max);
    let q = sigma * (v / l).powi(2);
    let exact = 20.0 + q * l * l / (8.0 * k);
    assert!((peak - exact).abs() < 1e-9 * exact, "{peak} {exact}");
}

/// The bar of conductivity 1e3 (R0 = 1 ohm) with a very large thermal conductivity, so its
/// temperature is uniform, and a convection of 1000 W/(m2 K) to 0 C on its 6 faces (h A =
/// 4.2 W/K). With alpha, R = R0 (1 + alpha T) and T = P / (h A).
fn hot_bar(left: Drive) -> Model {
    let mut model = bar(0.1, 1e3, left, Drive::Voltage(0.0));
    model.materials[0].coefficient = 0.01;
    model.heat = Some(heat_of(
        &model,
        1e9,
        vec![Load::Convection {
            faces: 2,
            film: 1000.0,
            sink: 0.0,
        }],
    ));
    model
}

#[test]
fn a_temperature_dependent_conductivity_converges_to_the_fixed_point() {
    // Current drive I = 10 A: T = I^2 R0 (1 + alpha T) / (h A).
    let s = run(&hot_bar(Drive::Current(10.0)));
    let exact = 100.0 / 4.2 / (1.0 - 100.0 * 0.01 / 4.2);
    let t = s.temperature.unwrap();
    for t in &t {
        assert!((t / exact - 1.0).abs() < 1e-4, "{t} {exact}");
    }
    let r = s.terminals[0].0 / s.terminals[0].1;
    assert!((r / (1.0 + 0.01 * exact) - 1.0).abs() < 1e-4, "{r}");
    assert!(s.iterations > 2, "{}", s.iterations);
}

#[test]
fn thermal_runaway_is_refused() {
    // I^2 R0 alpha / (h A) = 900 * 0.01 / 4.2 > 1: the temperature grows with no limit.
    let e = solve(&hot_bar(Drive::Current(30.0)), &mut Quiet).unwrap_err();
    assert!(e.contains("thermal runaway"), "{e}");
}

#[test]
fn a_model_with_no_voltage_is_refused() {
    let e = solve(
        &bar(0.3, 2.0, Drive::Current(1.0), Drive::Current(-1.0)),
        &mut Quiet,
    )
    .unwrap_err();
    assert!(e.contains("reference of the potential"), "{e}");
}

#[test]
fn a_floating_conductor_has_no_charge_and_the_mean_potential() {
    // Plates at 1 V and 0 V on the ends of a dielectric bar, and a floating plate on the middle
    // of its y- side: by the antisymmetry about x = 0.15, its potential is 0.5 V.
    let mut model = bar(0.3, 4.0, Drive::Voltage(1.0), Drive::Voltage(0.0));
    model.kind = Kind::Electrostatic;
    model.faces.push(FaceSet {
        name: "float".into(),
        sides: vec![Side::parse("y-").unwrap()],
        region: Some(([0.1, -1.0, -1.0], [0.2, 1.0, 1.0])),
        parts: vec![],
        circle: None,
    });
    model.terminals.push(Terminal {
        faces: 3,
        drive: Drive::Current(0.0),
    });
    let s = run(&model);
    let (v, q) = s.terminals[2];
    assert!((v - 0.5).abs() < 1e-9 && q.abs() < 1e-20, "{v} {q}");
    // The charges add to 0, and the floating plate shortens the gap: C > eps0 eps_r A / L.
    let total: f64 = s.terminals.iter().map(|t| t.1).sum();
    assert!(total.abs() < 1e-9 * s.terminals[0].1, "{total}");
    assert!(s.terminals[0].1 > fh_fem::dc::EPS0 * 4.0 * 1e-4 / 0.3);
}
