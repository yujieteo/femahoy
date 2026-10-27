//! Frequency-domain electromagnetics on the extruded tetrahedral mesh against exact answers: the
//! TE10 wave of a rectangular waveguide, the reflection of a dielectric section, an absorbing
//! layer, the TEM wave of a coaxial line and the modes of a rectangular cavity.

use fh_fem::complex::C64;
use fh_fem::em::{solve, Boundary, Events, Frame, Material, Mode, Model, Port, Step, Wall, LIGHT};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{FaceSet, Mesh};
use fh_fem::prism::{Footprint, Outline};

#[derive(Default)]
struct Record {
    frames: Vec<Frame>,
    lines: Vec<String>,
}

impl Events for Record {
    fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, text: &str) {
        self.lines.push(text.to_owned());
    }
    fn frame(&mut self, frame: &Frame) -> Result<(), String> {
        self.frames.push(frame.clone());
        Ok(())
    }
}

fn material(name: &str, permittivity: f64) -> Material {
    Material {
        name: name.into(),
        permittivity,
        loss_tangent: 0.0,
        permeability: 1.0,
        conductivity: 0.0,
        perfect: false,
    }
}

fn boxed(name: &str, material: usize, lo: [f64; 3], hi: [f64; 3]) -> Part {
    Part {
        name: name.into(),
        material,
        priority: 0,
        lo,
        hi,
        footprint: None,
    }
}

fn side(name: &str, axis: usize, positive: bool) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: vec![Side { axis, positive }],
        region: None,
        parts: Vec::new(),
        circle: None,
    }
}

fn spacing(max_cell: f64) -> Spacing {
    Spacing {
        max_cell,
        max_growth: 1.3,
        min_cells: 2,
    }
}

const A: f64 = 22.86e-3;
const B: f64 = 10.16e-3;

/// A WR-90 waveguide along z from 0 to `length`, with the ports at both ends.
fn waveguide(length: f64, max_cell: f64, f: f64) -> Model {
    Model {
        parts: vec![boxed("guide", 0, [0.0; 3], [A, B, length])],
        materials: vec![material("air", 1.0)],
        spacing: spacing(max_cell),
        axis: 2,
        length: 1.0,
        pml: vec![false],
        faces: vec![side("in", 2, false), side("out", 2, true)],
        walls: Vec::new(),
        ports: vec![
            Port {
                faces: 0,
                mode: Mode::Te10,
                deembed: 0.0,
            },
            Port {
                faces: 1,
                mode: Mode::Te10,
                deembed: 0.0,
            },
        ],
        step: Step::Frequency {
            start: f,
            stop: f,
            points: 1,
            output_every: 1,
        },
    }
}

fn beta_te10(f: f64, eps: f64) -> f64 {
    let k = 2.0 * std::f64::consts::PI * f / LIGHT;
    (k * k * eps - (std::f64::consts::PI / A).powi(2)).sqrt()
}

/// The angle in (-pi, pi].
fn wrap(t: f64) -> f64 {
    let two = 2.0 * std::f64::consts::PI;
    let r = t.rem_euclid(two);
    if r > std::f64::consts::PI {
        r - two
    } else {
        r
    }
}

#[test]
fn a_waveguide_carries_te10_with_its_exact_propagation_constant() {
    let (f, length) = (10e9, 30e-3);
    let mut r = Record::default();
    solve(&waveguide(length, 2.0e-3, f), &mut r).unwrap();
    let s = &r.frames[0].s;
    let (s11, s21) = (s[0], s[2]);
    let beta = beta_te10(f, 1.0);
    let phase_error = wrap(s21.arg() + beta * length);
    println!(
        "{:?} s11 {} s21 {} phase error {phase_error}",
        r.lines,
        s11.abs(),
        s21.abs()
    );
    assert!(s11.abs() < 0.01, "|S11| {}", s11.abs());
    assert!((s21.abs() - 1.0).abs() < 0.002, "|S21| {}", s21.abs());
    assert!(phase_error.abs() < 0.01, "phase error {phase_error}");
    assert!((s[1] - s[2]).abs() < 1e-9, "reciprocity");
    assert!((r.frames[0].ports[0].beta - beta).abs() < 1e-9 * beta);
}

#[test]
fn a_rectangular_cavity_resonates_at_its_exact_frequency() {
    let (a, b, d) = (22.86e-3, 10.16e-3, 25.0e-3);
    let model = Model {
        parts: vec![boxed("cavity", 0, [0.0; 3], [a, b, d])],
        materials: vec![material("air", 1.0)],
        spacing: spacing(2.0e-3),
        axis: 2,
        length: 1.0,
        pml: vec![false],
        faces: Vec::new(),
        walls: Vec::new(),
        ports: Vec::new(),
        step: Step::Modes {
            near: 8e9,
            count: 2,
        },
    };
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    let exact = |m: f64, n: f64, l: f64| {
        LIGHT / 2.0 * ((m / a).powi(2) + (n / b).powi(2) + (l / d).powi(2)).sqrt()
    };
    let te101 = exact(1.0, 0.0, 1.0);
    let got: Vec<f64> = r.frames.iter().map(|f| f.frequency).collect();
    println!(
        "{:?} modes {got:?} exact TE101 {te101} TE102 {}",
        r.lines,
        exact(1.0, 0.0, 2.0)
    );
    assert!(
        (got[0] - te101).abs() < 2e-3 * te101,
        "{got:?} against {te101}"
    );
}

/// Reason for the band of 1 %: at max_cell 2 mm the Q is 0.41 % below the exact Q, and the error
/// falls at second order (validation/cases/grid-cavity-te101/derivation.md).
#[test]
fn a_cavity_with_copper_walls_has_the_q_of_its_surface_resistance() {
    let (a, b, d, sigma) = (22.86e-3, 10.16e-3, 25.0e-3, 5.8e7);
    let all = FaceSet {
        name: "walls".into(),
        sides: (0..3)
            .flat_map(|axis| [false, true].map(|positive| Side { axis, positive }))
            .collect(),
        region: None,
        parts: Vec::new(),
        circle: None,
    };
    let model = Model {
        parts: vec![boxed("cavity", 0, [0.0; 3], [a, b, d])],
        materials: vec![material("air", 1.0)],
        spacing: spacing(2.0e-3),
        axis: 2,
        length: 1.0,
        pml: vec![false],
        faces: vec![all],
        walls: vec![Boundary {
            faces: 0,
            wall: Wall::Conductor(sigma),
        }],
        ports: Vec::new(),
        step: Step::Modes {
            near: 8e9,
            count: 1,
        },
    };
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    // Pozar, Microwave Engineering, 4th ed., section 6.3: the Q of TE101.
    let f = LIGHT / 2.0 * ((1.0 / a).powi(2) + (1.0 / d).powi(2)).sqrt();
    let mu0 = 4e-7 * std::f64::consts::PI;
    let k = 2.0 * std::f64::consts::PI * f / LIGHT;
    let rs = (std::f64::consts::PI * f * mu0 / sigma).sqrt();
    let q = (k * a * d).powi(3) * b * (mu0 * LIGHT)
        / (2.0 * std::f64::consts::PI.powi(2) * rs)
        / (2.0 * a.powi(3) * b + 2.0 * b * d.powi(3) + a.powi(3) * d + a * d.powi(3));
    let got = r.frames[0].q.unwrap();
    println!("{:?} q {got} exact {q}", r.lines);
    assert!((got - q).abs() < 0.01 * q, "Q {got} against {q}");
}

#[test]
fn an_absorbing_layer_ends_a_waveguide_with_a_small_reflection() {
    let f = 10e9;
    let mut model = waveguide(20e-3, 2.0e-3, f);
    // A layer of 1 guide wavelength after the guide.
    let lg = 2.0 * std::f64::consts::PI / beta_te10(f, 1.0);
    model
        .parts
        .push(boxed("pml", 0, [0.0, 0.0, 20e-3], [A, B, 20e-3 + lg]));
    model.pml.push(true);
    model.ports.pop();
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    let s11 = r.frames[0].s[0];
    println!("{:?} |S11| {}", r.lines, s11.abs());
    assert!(s11.abs() < 0.02, "|S11| {}", s11.abs());
}

#[test]
fn a_dielectric_section_reflects_as_a_transmission_line() {
    // A section of eps 2.2 from z = 10 to 20 mm in a guide of 30 mm.
    let f = 10e9;
    let mut model = waveguide(30e-3, 1.5e-3, f);
    model.materials.push(material("ptfe", 2.2));
    model.parts.push(Part {
        priority: 1,
        ..boxed("slab", 1, [0.0, 0.0, 10e-3], [A, B, 20e-3])
    });
    model.pml.push(false);
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    // The exact answer: a line of length l with the wave impedance Z2 between 2 lines Z1.
    let (b1, b2) = (beta_te10(f, 1.0), beta_te10(f, 2.2));
    let (z1, z2) = (1.0 / b1, 1.0 / b2);
    let l = 10e-3;
    let zin = {
        let t = C64::new(0.0, (b2 * l).tan());
        C64::from(z2) * (C64::from(z1) + t * z2) / (C64::from(z2) + t * z1)
    };
    let gamma = (zin - C64::from(z1)) / (zin + C64::from(z1));
    // The reference planes are the ports, 10 mm before and after the section.
    let s11_exact = gamma * C64::cis(-2.0 * b1 * 10e-3);
    let s11 = r.frames[0].s[0];
    println!("{:?} s11 {s11:?} exact {s11_exact:?}", r.lines);
    assert!(
        (s11 - s11_exact).abs() < 0.01,
        "{s11:?} against {s11_exact:?}"
    );
}

#[test]
fn a_coaxial_line_has_its_exact_impedance_and_wave() {
    // Inner radius 1 mm, outer 2.3 mm, eps 2.1, along z for 20 mm.
    let (ri, ro, eps, length) = (1.0e-3, 2.3e-3, 2.1, 20e-3);
    let f = 5e9;
    let part = Part {
        name: "dielectric".into(),
        material: 0,
        priority: 0,
        lo: [0.0, 0.0, 0.0],
        hi: [0.0, 0.0, length],
        footprint: Some(Footprint {
            outline: Outline::Circle {
                centre: [0.0, 0.0],
                radius: ro,
            },
            holes: vec![Outline::Circle {
                centre: [0.0, 0.0],
                radius: ri,
            }],
        }),
    };
    let model = Model {
        parts: vec![part],
        materials: vec![material("ptfe", eps)],
        spacing: spacing(0.5e-3),
        axis: 2,
        length: 1.0,
        pml: vec![false],
        faces: vec![
            side("in", 2, false),
            side("out", 2, true),
            FaceSet {
                name: "inner".into(),
                sides: Vec::new(),
                region: None,
                parts: Vec::new(),
                circle: Some(([0.0, 0.0], ri)),
            },
        ],
        walls: Vec::new(),
        ports: vec![
            Port {
                faces: 0,
                mode: Mode::Tem { signal: 2 },
                deembed: 0.0,
            },
            Port {
                faces: 1,
                mode: Mode::Tem { signal: 2 },
                deembed: 0.0,
            },
        ],
        step: Step::Frequency {
            start: f,
            stop: f,
            points: 1,
            output_every: 1,
        },
    };
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    let eta0 = 376.730313668;
    let z0 = eta0 / (2.0 * std::f64::consts::PI * eps.sqrt()) * (ro / ri).ln();
    let fr = &r.frames[0];
    let (s11, s21) = (fr.s[0], fr.s[2]);
    let beta = 2.0 * std::f64::consts::PI * f / LIGHT * eps.sqrt();
    println!(
        "{:?} z {} exact {z0} beta {} exact {beta} s11 {} s21 {} phase error {}",
        r.lines,
        fr.ports[0].impedance,
        fr.ports[0].beta,
        s11.abs(),
        s21.abs(),
        wrap(s21.arg() + beta * length)
    );
    assert!((fr.ports[0].impedance - z0).abs() < 0.01 * z0);
    assert!((fr.ports[0].beta - beta).abs() < 1e-9 * beta);
    assert!(s11.abs() < 0.01 && (s21.abs() - 1.0).abs() < 0.002);
}

/// The time of the direct solver on a waveguide of growing size (run with `--ignored
/// --nocapture`): the measurement behind `em::LAPTOP`.
#[test]
#[ignore]
fn direct_cost() {
    for max_cell in [2.0e-3, 1.4e-3, 1.0e-3] {
        let model = waveguide(60e-3, max_cell, 10e9);
        let mut r = Record::default();
        let start = std::time::Instant::now();
        solve(&model, &mut r).unwrap();
        println!("{:.1} s: {}", start.elapsed().as_secs_f64(), r.lines[0]);
    }
}

/// The iterative solver against the direct solver on the waveguide with the PTFE section.
#[test]
fn the_iterative_solver_gives_the_s_parameters_of_the_direct_solver() {
    let f = 10e9;
    let mut model = waveguide(30e-3, 1.5e-3, f);
    model.materials.push(material("ptfe", 2.2));
    model.parts.push(Part {
        priority: 1,
        ..boxed("slab", 1, [0.0, 0.0, 10e-3], [A, B, 20e-3])
    });
    model.pml.push(false);
    let mut direct = Record::default();
    solve(&model, &mut direct).unwrap();
    let limit = fh_fem::em::Limit {
        direct: 0,
        ..fh_fem::em::LAPTOP
    };
    let mut iterative = Record::default();
    fh_fem::em::solve_within(&model, limit, &mut iterative).unwrap();
    println!("{:?}", iterative.lines);
    for (a, b) in direct.frames[0].s.iter().zip(&iterative.frames[0].s) {
        assert!((*a - *b).abs() < 1e-6, "{a:?} against {b:?}");
    }
}

/// The time of the iterative solver against the direct solver on the waveguide with the PTFE
/// section, on growing meshes (run with `--ignored --nocapture`): the measurement behind the
/// limit `direct` of `em::LAPTOP`.
#[test]
#[ignore]
fn iterative_cost() {
    let cells: Vec<f64> = std::env::var("CELLS")
        .unwrap_or("1.5 1.0".into())
        .split_whitespace()
        .map(|c| c.parse::<f64>().unwrap() * 1e-3)
        .collect();
    for max_cell in cells {
        let mut model = waveguide(30e-3, max_cell, 10e9);
        model.materials.push(material("ptfe", 2.2));
        model.parts.push(Part {
            priority: 1,
            ..boxed("slab", 1, [0.0, 0.0, 10e-3], [A, B, 20e-3])
        });
        model.pml.push(false);
        for direct in [usize::MAX, 0] {
            if direct == usize::MAX && std::env::var("SKIP_DIRECT").is_ok() {
                continue;
            }
            let limit = fh_fem::em::Limit {
                direct,
                points: usize::MAX,
                ..fh_fem::em::LAPTOP
            };
            let mut r = Record::default();
            let start = std::time::Instant::now();
            fh_fem::em::solve_within(&model, limit, &mut r).unwrap();
            println!(
                "{:.1} s: {:?} s21 {:?}",
                start.elapsed().as_secs_f64(),
                r.lines,
                r.frames[0].s[2]
            );
        }
    }
}
