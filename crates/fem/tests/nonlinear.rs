//! Nonlinear heat conduction: a radiation to the ambient and a conductivity and a specific heat
//! that change with the temperature, against exact answers of a slab, and the energy balance of
//! a transient step.

use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{solve, Events, FaceSet, Frame, Load, Material, Mesh, Model, Probe, Step};
use fh_fem::nonlinear::Table;
use fh_fem::prism::{Footprint, Outline};
use fh_fem::solve::Method;

const SIGMA: f64 = 5.670374419e-8;

#[derive(Default)]
struct Record {
    frames: Vec<Frame>,
    progress: Vec<String>,
}

impl Events for Record {
    fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, line: &str) {
        self.progress.push(line.into());
    }
    fn frame(&mut self, frame: &Frame) -> Result<(), String> {
        self.frames.push(frame.clone());
        Ok(())
    }
}

fn run(model: &Model) -> (f64, Vec<Frame>) {
    let mut r = Record::default();
    let s = solve(model, &mut r).unwrap();
    (s.imbalance, r.frames)
}

fn material(k: f64) -> Material {
    Material {
        name: "solid".into(),
        conductivity: [k; 3],
        capacity: 0.0,
        conductivity_table: None,
        capacity_table: None,
    }
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

fn probe(name: &str, z: f64) -> Probe {
    Probe {
        name: name.into(),
        point: [0.005, 0.005, z],
    }
}

/// A bar 0.01 x 0.01 across and `length` along z, with cells of `cell` along z.
fn slab(length: f64, cell: f64, material: Material, loads: Vec<Load>) -> Model {
    Model {
        parts: vec![Part {
            name: "slab".into(),
            material: 0,
            priority: 0,
            lo: [0.0; 3],
            hi: [0.01, 0.01, length],
            footprint: None,
        }],
        materials: vec![material],
        spacing: Spacing {
            max_cell: cell,
            max_growth: 1.3,
            min_cells: 2,
        },
        axis: 2,
        symmetry: [false; 3],
        faces: vec![side("bottom", "z-"), side("top", "z+")],
        loads,
        step: Step::Steady,
        probes: vec![probe("top", length), probe("middle", 0.5 * length)],
        method: None,
    }
}

fn radiation(faces: usize, emissivity: f64, ambient: f64) -> Load {
    Load::Radiation {
        faces,
        emissivity,
        ambient,
        stefan_boltzmann: SIGMA,
    }
}

/// The root T2 of k (t1 - T2) / length = eps sigma (T2^4 - ambient^4), by bisection.
fn face_temperature(k: f64, length: f64, t1: f64, eps: f64, ambient: f64) -> f64 {
    let f = |t: f64| k * (t1 - t) / length - eps * SIGMA * (t.powi(4) - ambient.powi(4));
    let (mut lo, mut hi) = (ambient, t1);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if f(mid) > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

#[test]
fn a_slab_that_radiates_has_the_root_of_its_energy_balance() {
    let (k, length, t1, eps, ambient) = (10.0, 0.1, 1000.0, 0.8, 300.0);
    let exact = face_temperature(k, length, t1, eps, ambient);
    let loads = vec![
        Load::Temperature {
            faces: 0,
            value: t1,
        },
        radiation(1, eps, ambient),
    ];
    let mut box_model = slab(length, 0.01, material(k), loads);
    let (imbalance, frames) = run(&box_model);
    let t2 = frames[0].probes[0];
    assert!((t2 - exact).abs() < 1e-8 * exact, "{t2} against {exact}");
    assert!(imbalance < 1e-9, "{imbalance}");
    let e = frames[0].energy;
    let area = 0.01 * 0.01;
    let q = eps * SIGMA * (exact.powi(4) - ambient.powi(4)) * area;
    assert!(
        (e.radiation + q).abs() < 1e-8 * q,
        "{} against {}",
        e.radiation,
        -q
    );
    // The same slab on the mesh of tetrahedra, and with multigrid on a finer grid.
    box_model.parts[0].footprint = Some(Footprint {
        outline: Outline::Polygon(vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]]),
        holes: vec![],
    });
    box_model.parts[0].lo = [0.0; 3];
    box_model.parts[0].hi = [0.0, 0.0, length];
    let (_, frames) = run(&box_model);
    let t2 = frames[0].probes[0];
    assert!(
        (t2 - exact).abs() < 1e-8 * exact,
        "tet: {t2} against {exact}"
    );
    let mut fine = slab(length, 0.0005, material(k), box_model.loads.clone());
    fine.method = Some(Method::Multigrid);
    let (_, frames) = run(&fine);
    let t2 = frames[0].probes[0];
    assert!(
        (t2 - exact).abs() < 1e-8 * exact,
        "multigrid: {t2} against {exact}"
    );
}

#[test]
fn a_conductivity_linear_in_the_temperature_gives_the_kirchhoff_profile() {
    // k = k0 (1 + beta T): Phi(T) = k0 (T + beta T^2 / 2) is linear in z.
    let (k0, beta, length, t1, t2) = (2.0, 2e-3, 1.0, 500.0, 300.0);
    let table = Table {
        temperature: vec![t2, t1],
        value: vec![k0 * (1.0 + beta * t2), k0 * (1.0 + beta * t1)],
    };
    let phi = |t: f64| k0 * (t + 0.5 * beta * t * t);
    let exact = |z: f64| {
        let p = phi(t1) + (phi(t2) - phi(t1)) * z / length;
        (-1.0 + (1.0 + 2.0 * beta * p / k0).sqrt()) / beta
    };
    let mut m = material(1.0);
    m.conductivity_table = Some(table);
    let loads = vec![
        Load::Temperature {
            faces: 0,
            value: t1,
        },
        Load::Temperature {
            faces: 1,
            value: t2,
        },
    ];
    let mut model = slab(length, length / 20.0, m, loads);
    model.probes = vec![probe("a", 0.25), probe("b", 0.5), probe("c", 0.8)];
    let (imbalance, frames) = run(&model);
    for (p, z) in frames[0].probes.iter().zip([0.25, 0.5, 0.8]) {
        assert!(
            (p - exact(z)).abs() < 1e-8 * exact(z),
            "{p} against {}",
            exact(z)
        );
    }
    assert!(imbalance < 1e-8, "{imbalance}");
    // The flux of each cell is the Kirchhoff flux (Phi(t1) - Phi(t2)) / length.
    let q = (phi(t1) - phi(t2)) / length;
    for f in &frames[0].flux {
        assert!((f[2] - q).abs() < 1e-7 * q, "{} against {q}", f[2]);
    }
}

#[test]
fn a_strong_conductivity_converges_quadratically() {
    // k = k0 (1 + beta T) with beta T from 2 to 5: a tangent with no dk/dT term converges
    // slowly here.
    let (k0, beta, length, t1, t2) = (2.0, 0.01, 1.0, 500.0, 200.0);
    let table = Table {
        temperature: vec![t2, t1],
        value: vec![k0 * (1.0 + beta * t2), k0 * (1.0 + beta * t1)],
    };
    let phi = |t: f64| k0 * (t + 0.5 * beta * t * t);
    let exact = |z: f64| {
        let p = phi(t1) + (phi(t2) - phi(t1)) * z / length;
        (-1.0 + (1.0 + 2.0 * beta * p / k0).sqrt()) / beta
    };
    let mut m = material(1.0);
    m.conductivity_table = Some(table);
    let loads = vec![
        Load::Temperature {
            faces: 0,
            value: t1,
        },
        Load::Temperature {
            faces: 1,
            value: t2,
        },
    ];
    let mut model = slab(length, length / 20.0, m, loads);
    model.probes = vec![probe("a", 0.25), probe("b", 0.5), probe("c", 0.8)];
    let mut r = Record::default();
    solve(&model, &mut r).unwrap();
    for (p, z) in r.frames[0].probes.iter().zip([0.25, 0.5, 0.8]) {
        assert!(
            (p - exact(z)).abs() < 1e-8 * exact(z),
            "{p} against {}",
            exact(z)
        );
    }
    let iterations: usize = r
        .progress
        .iter()
        .find_map(|l| l.split("Newton ").nth(1)?.split(' ').next()?.parse().ok())
        .expect("a progress line with the Newton iterations");
    assert!(iterations <= 8, "{iterations} Newton iterations");
}

#[test]
fn a_transient_step_with_tables_and_radiation_keeps_the_energy_balance() {
    let mut m = material(1.0);
    m.conductivity_table = Some(Table {
        temperature: vec![300.0, 1000.0],
        value: vec![20.0, 12.0],
    });
    m.capacity_table = Some(Table {
        temperature: vec![300.0, 600.0, 1000.0],
        value: vec![3.5e6, 4.0e6, 5.0e6],
    });
    let loads = vec![
        Load::Flux {
            faces: 0,
            value: 2e5,
        },
        radiation(1, 0.9, 300.0),
    ];
    let mut model = slab(0.02, 0.002, m, loads);
    model.step = Step::Transient {
        end: 50.0,
        dt: 1.0,
        initial: 300.0,
        output_every: 10,
    };
    let (imbalance, frames) = run(&model);
    assert!(imbalance < 1e-8, "{imbalance}");
    let last = frames.last().unwrap();
    assert!(last.probes[0] > 300.0 && last.energy.radiation < 0.0);
    assert!(last.energy.stored > 0.0);
}

#[test]
fn a_body_at_equilibrium_with_its_ambient_stays_there() {
    // No load and no fixed temperature: each heat flow is 0, and only the rounding of K(T) T
    // is left in the residual.
    let mut m = material(1.0);
    m.capacity = 4e6;
    m.conductivity_table = Some(Table {
        temperature: vec![300.0, 1000.0],
        value: vec![20.0, 12.0],
    });
    let mut model = slab(0.02, 0.002, m, vec![radiation(1, 0.9, 300.0)]);
    model.step = Step::Transient {
        end: 5.0,
        dt: 1.0,
        initial: 300.0,
        output_every: 1,
    };
    let (_, frames) = run(&model);
    for p in &frames.last().unwrap().probes {
        assert!((p - 300.0).abs() < 1e-9, "{p}");
    }
}

#[test]
fn a_constant_table_gives_the_linear_transient() {
    let loads = vec![
        Load::Temperature {
            faces: 0,
            value: 400.0,
        },
        Load::Convection {
            faces: 1,
            film: 50.0,
            sink: 300.0,
        },
    ];
    let mut linear = slab(0.02, 0.002, material(15.0), loads);
    linear.materials[0].capacity = 4e6;
    linear.step = Step::Transient {
        end: 20.0,
        dt: 0.5,
        initial: 300.0,
        output_every: 40,
    };
    let mut tables = linear.clone();
    tables.materials[0].conductivity_table = Some(Table {
        temperature: vec![0.0, 1000.0],
        value: vec![15.0, 15.0],
    });
    tables.materials[0].capacity_table = Some(Table {
        temperature: vec![0.0, 1000.0],
        value: vec![4e6, 4e6],
    });
    let (_, a) = run(&linear);
    let (_, b) = run(&tables);
    for (x, y) in a
        .last()
        .unwrap()
        .probes
        .iter()
        .zip(&b.last().unwrap().probes)
    {
        assert!((x - y).abs() < 1e-9 * x, "{x} against {y}");
    }
}

#[test]
fn bad_tables_and_radiations_are_refused() {
    let mut m = material(1.0);
    m.conductivity_table = Some(Table {
        temperature: vec![500.0, 300.0],
        value: vec![1.0, 2.0],
    });
    let loads = vec![radiation(1, 0.5, 300.0)];
    let model = slab(0.1, 0.01, m, loads);
    let err = solve(&model, &mut Record::default()).unwrap_err();
    assert!(err.contains("must increase"), "{err}");
    let model = slab(0.1, 0.01, material(1.0), vec![radiation(1, 0.5, 0.0)]);
    let err = solve(&model, &mut Record::default()).unwrap_err();
    assert!(err.contains("kelvin"), "{err}");
}
