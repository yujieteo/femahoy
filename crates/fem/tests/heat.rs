//! Heat conduction on the box grid: exact linear fields, the energy balance, symmetry, the 2
//! linear solvers and the refusal of a model with no unique solution.

use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{
    solve, Events, FaceSet, Frame, Load, Material, Mesh, Model, Probe, Step, Summary,
};
use fh_fem::solve::Method;

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

fn run(model: &Model) -> (Summary, Record) {
    let mut r = Record::default();
    let s = solve(model, &mut r).unwrap();
    (s, r)
}

fn material(name: &str, k: f64) -> Material {
    Material {
        name: name.into(),
        conductivity: [k; 3],
        capacity: 1.0,
    }
}

fn part(name: &str, material: usize, lo: [f64; 3], hi: [f64; 3]) -> Part {
    Part {
        name: name.into(),
        material,
        priority: 0,
        lo,
        hi,
    }
}

fn side(name: &str, s: &str) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: vec![Side::parse(s).unwrap()],
        region: None,
        parts: vec![],
    }
}

/// A wall of 3 layers along x, 0.1 x 0.1 in y and z, with T = 100 at x = 0 and T = 0 at x = 0.6.
fn wall() -> Model {
    Model {
        parts: vec![
            part("a", 0, [0.0, 0.0, 0.0], [0.1, 0.1, 0.1]),
            part("b", 1, [0.1, 0.0, 0.0], [0.4, 0.1, 0.1]),
            part("c", 2, [0.4, 0.0, 0.0], [0.6, 0.1, 0.1]),
        ],
        materials: vec![
            material("m1", 1.0),
            material("m2", 0.1),
            material("m3", 2.0),
        ],
        spacing: Spacing {
            max_cell: 0.025,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("hot", "x-"), side("cold", "x+")],
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
        probes: vec![
            Probe {
                name: "ab".into(),
                point: [0.1, 0.05, 0.05],
            },
            Probe {
                name: "bc".into(),
                point: [0.4, 0.05, 0.05],
            },
        ],
        method: None,
    }
}

#[test]
fn a_layered_wall_has_the_exact_interface_temperatures_and_heat_flow() {
    let (s, r) = run(&wall());
    // Resistances per area: 0.1 / 1 + 0.3 / 0.1 + 0.2 / 2 = 3.2. q = 100 / 3.2 = 31.25.
    let q = 100.0 / 3.2;
    let f = &r.frames[0];
    assert!(
        (f.probes[0] - (100.0 - q * 0.1)).abs() < 1e-8,
        "{}",
        f.probes[0]
    );
    assert!((f.probes[1] - q * 0.1).abs() < 1e-8, "{}", f.probes[1]);
    // The heat in at x = 0 is q times the area 0.01, and it leaves at x = 0.6.
    assert!(f.energy.fixed.abs() < 1e-9);
    assert!(s.imbalance < 1e-9);
    assert!(f.flux.iter().all(|v| (v[0] - q).abs() < 1e-6));
}

#[test]
fn the_direct_and_the_multigrid_solvers_agree() {
    let mut m = wall();
    // A finer grid with a convection, so the field is 3D.
    m.spacing.max_cell = 0.01;
    m.faces.push(side("top", "z+"));
    m.loads[1] = Load::Convection {
        faces: 2,
        film: 50.0,
        sink: 20.0,
    };
    m.loads.push(Load::Source {
        part: 1,
        value: 1e4,
    });
    m.method = Some(Method::Direct);
    let (_, direct) = run(&m);
    m.method = Some(Method::Multigrid);
    let (s, iterative) = run(&m);
    assert_eq!(s.method, Method::Multigrid);
    let (a, b) = (&direct.frames[0], &iterative.frames[0]);
    let diff = a
        .temperature
        .iter()
        .zip(&b.temperature)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f64::max);
    assert!(diff < 1e-6, "{diff}");
    assert!(s.imbalance < 1e-8, "{}", s.imbalance);
    // The heat of the source leaves through the fixed face and the convection.
    let e = b.energy;
    assert!((e.source - 1e4 * 0.3 * 0.01).abs() < 1e-9);
    assert!((e.source + e.fixed + e.convection).abs() < 1e-6 * e.source);
}

#[test]
fn a_half_model_gives_the_temperatures_of_the_full_model() {
    // A plate heated in its middle, cooled on its top, symmetric in x and y.
    let model = |symmetry| Model {
        parts: vec![
            part("plate", 0, [0.0, 0.0, 0.0], [1.0, 0.6, 0.05]),
            part("heater", 1, [0.4, 0.2, 0.05], [0.6, 0.4, 0.1]),
        ],
        materials: vec![material("al", 200.0), material("si", 150.0)],
        spacing: Spacing {
            max_cell: 0.05,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry,
        faces: vec![side("bottom", "z-")],
        loads: vec![
            Load::Convection {
                faces: 0,
                film: 1000.0,
                sink: 25.0,
            },
            Load::Source {
                part: 1,
                value: 1e6,
            },
        ],
        step: Step::Steady,
        probes: vec![
            Probe {
                name: "centre".into(),
                point: [0.5, 0.3, 0.1],
            },
            Probe {
                name: "corner".into(),
                point: [0.0, 0.0, 0.0],
            },
            Probe {
                name: "mirror".into(),
                point: [1.0, 0.6, 0.0],
            },
        ],
        method: None,
    };
    let (_, full) = run(&model([false; 3]));
    let (s, quarter) = run(&model([true, true, false]));
    assert!(s.warnings.iter().any(|w| w.contains("symmetry")));
    let (a, b) = (&full.frames[0].probes, &quarter.frames[0].probes);
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-8 * a[i].abs(), "{a:?} {b:?}");
    }
    assert!(quarter.mesh.unwrap().cells.len() * 4 == full.mesh.unwrap().cells.len());
}

#[test]
fn symmetry_of_a_model_that_does_not_mirror_is_refused() {
    let mut m = wall();
    m.symmetry = [true, false, false];
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("do not mirror"), "{e}");
}

#[test]
fn a_steady_part_with_no_fixed_temperature_or_convection_is_refused() {
    let mut m = wall();
    // A block away from the wall, with only a heat source.
    m.parts
        .push(part("loose", 0, [2.0, 0.0, 0.0], [2.1, 0.1, 0.1]));
    m.faces[0].parts = vec![0];
    m.faces[1].parts = vec![2];
    m.loads.push(Load::Source {
        part: 3,
        value: 1.0,
    });
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("part loose has no fixed temperature"), "{e}");
}

#[test]
fn a_transient_ends_at_the_steady_state_with_its_energy_balance() {
    let mut m = wall();
    m.step = Step::Transient {
        end: 20.0,
        dt: 0.1,
        initial: 0.0,
        output_every: 5,
    };
    let (s, r) = run(&m);
    assert_eq!(r.frames.len(), 41);
    assert!(s.imbalance < 1e-9, "{}", s.imbalance);
    let last = r.frames.last().unwrap();
    let q = 100.0 / 3.2;
    assert!(
        (last.probes[0] - (100.0 - q * 0.1)).abs() < 1e-3,
        "{}",
        last.probes[0]
    );
    // In the first steps the wall stores heat.
    assert!(r.frames[1].energy.stored > 0.0);
}

/// A plate of 5 layers with a conductivity ratio of 4000 and layers 0.02 to 2 thick, heated by
/// a block on its top and cooled on its bottom. The cells of the thin layers are flat, which is
/// the hard case of the multigrid smoother.
fn laminate(n: usize) -> Model {
    let layers = [
        (0.0, 2.0, 0),
        (2.0, 2.02, 1),
        (2.02, 2.5, 2),
        (2.5, 2.52, 1),
        (2.52, 3.0, 3),
    ];
    let mut parts: Vec<Part> = layers
        .iter()
        .enumerate()
        .map(|(i, &(z0, z1, m))| part(&format!("layer{i}"), m, [0.0, 0.0, z0], [20.0, 20.0, z1]))
        .collect();
    parts.push(part("block", 0, [8.0, 8.0, 3.0], [12.0, 12.0, 3.5]));
    Model {
        parts,
        materials: vec![
            material("metal", 400.0),
            material("glue", 0.1),
            material("ceramic", 20.0),
            material("polymer", 0.3),
        ],
        spacing: Spacing {
            max_cell: 20.0 / n as f64,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("bottom", "z-")],
        loads: vec![
            Load::Convection {
                faces: 0,
                film: 1.0,
                sink: 0.0,
            },
            Load::Source {
                part: 5,
                value: 1.0,
            },
        ],
        step: Step::Steady,
        probes: vec![],
        method: None,
    }
}

/// A stiffened panel: a skin 0.002 thick in z with 4 ribs 0.003 thick in y, heated at 1 end
/// and cooled on the skin. Its thin parts are thin along 2 axes.
fn panel(n: usize) -> Model {
    let mut parts = vec![part("skin", 0, [0.0, 0.0, 0.0], [1.0, 0.5, 0.002])];
    for r in 0..4 {
        let y = 0.06 + 0.12 * r as f64;
        parts.push(part(
            &format!("rib{r}"),
            0,
            [0.0, y, 0.002],
            [1.0, y + 0.003, 0.04],
        ));
    }
    Model {
        parts,
        materials: vec![material("aluminium", 200.0)],
        spacing: Spacing {
            max_cell: 1.0 / n as f64,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("end", "x-"), side("skin", "z-")],
        loads: vec![
            Load::Flux {
                faces: 0,
                value: 1e4,
            },
            Load::Convection {
                faces: 1,
                film: 20.0,
                sink: 0.0,
            },
        ],
        step: Step::Steady,
        probes: vec![],
        method: None,
    }
}

/// Times of the direct solver and of PCG with multigrid, for the size limits of
/// `fh_fem::solve`: cubes of n^3 cells, the laminate, and the laminate with 100
/// time steps (1 factor or 1 multigrid setup, then 100 solves). Run with
/// `cargo test --release -p fh-fem --test heat crossover -- --ignored --nocapture`.
#[test]
#[ignore]
fn crossover() {
    let cube = |n: usize| {
        let mut m = wall();
        m.parts = vec![part("cube", 0, [0.0; 3], [1.0; 3])];
        m.materials = vec![material("m", 1.0)];
        m.spacing.max_cell = 1.0 / n as f64;
        m.faces = vec![side("hot", "x-"), side("top", "z+")];
        m.loads = vec![
            Load::Temperature {
                faces: 0,
                value: 0.0,
            },
            Load::Convection {
                faces: 1,
                film: 1.0,
                sink: 1.0,
            },
            Load::Source {
                part: 0,
                value: 1.0,
            },
        ];
        m.probes.clear();
        m
    };
    let time = |name: &str, m: &mut Model| {
        let mut out = Vec::new();
        for method in [Method::Direct, Method::Multigrid] {
            m.method = Some(method);
            let start = std::time::Instant::now();
            let (s, _) = run(m);
            out.push((s.unknowns, start.elapsed().as_secs_f64()));
        }
        println!(
            "{name} unknowns {} direct {:.3} s multigrid {:.3} s",
            out[0].0, out[0].1, out[1].1
        );
    };
    for n in [10, 16, 20, 25, 30, 36, 42, 50] {
        time(&format!("cube n {n}"), &mut cube(n));
    }
    for n in [10, 20, 40, 80] {
        time(&format!("panel n {n}"), &mut panel(n));
    }
    for n in [10, 20, 30, 40, 60] {
        time(&format!("laminate n {n}"), &mut laminate(n));
        let mut m = laminate(n);
        m.step = Step::Transient {
            end: 100.0,
            dt: 1.0,
            initial: 0.0,
            output_every: 100,
        };
        time(&format!("laminate transient n {n}"), &mut m);
    }
}
