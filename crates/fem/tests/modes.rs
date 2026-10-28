//! The natural frequencies of structures: a cantilever on the box grid and on the tetrahedral
//! mesh against beam theory, the 6 rigid modes of a free body, a repeated frequency, the 2
//! linear solvers and the refusals.

use fh_fem::eigen;
use fh_fem::elastic::{self, Load, Material, Thermal};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{FaceSet, Mesh};
use fh_fem::hex::Orthotropic;
use fh_fem::modes::{solve, solve_within, Events, Mode, Model, Summary};
use fh_fem::prism::{Footprint, Outline};
use fh_fem::solve::Method;

#[derive(Default)]
struct Record {
    modes: Vec<Mode>,
}

impl Events for Record {
    fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
        Ok(())
    }
    fn progress(&mut self, text: &str) {
        if std::env::var_os("FH_VERBOSE").is_some() {
            eprintln!("{text}");
        }
    }
    fn mode(&mut self, mode: &Mode) -> Result<(), String> {
        self.modes.push(mode.clone());
        Ok(())
    }
}

fn run(model: &Model, method: Option<Method>) -> (Summary, Vec<Mode>) {
    let mut r = Record::default();
    let s = solve_within(model, method, &mut r).unwrap();
    (s, r.modes)
}

/// Steel in SI-mm: MPa and t/mm3.
fn steel() -> Material {
    Material {
        name: "steel".into(),
        elastic: Orthotropic::isotropic(210e3, 0.3),
        expansion: [0.0; 3],
        density: 7.85e-9,
    }
}

fn face(name: &str, side: Side) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: vec![side],
        region: None,
        parts: Vec::new(),
        circle: None,
    }
}

/// A box of steel of the size `size` (mm), clamped at x = 0 when `clamped`.
fn beam(size: [f64; 3], max_cell: f64, clamped: bool, count: usize) -> Model {
    Model {
        structure: elastic::Model {
            parts: vec![Part {
                name: "beam".into(),
                material: 0,
                priority: 0,
                lo: [0.0; 3],
                hi: size,
                footprint: None,
            }],
            materials: vec![steel()],
            spacing: Spacing {
                max_cell,
                max_growth: 1.3,
                min_cells: 2,
            },
            axis: 2,
            symmetry: [false; 3],
            faces: vec![face("root", Side::ALL[0])],
            loads: if clamped {
                vec![Load::Displacement {
                    faces: 0,
                    value: [Some(0.0); 3],
                }]
            } else {
                Vec::new()
            },
            thermal: Thermal::None,
            reference: 0.0,
            probes: Vec::new(),
            surfaces: Vec::new(),
            method: None,
        },
        count,
    }
}

/// The first bending frequency of a clamped-free beam (Blevins, Table 8-1): lambda1 =
/// 1.87510407, f1 = lambda1^2 / (2 pi L^2) sqrt(E I / m).
fn cantilever_f1(length: f64, i: f64, area: f64) -> f64 {
    let lambda: f64 = 1.875_104_07;
    lambda * lambda / (2.0 * std::f64::consts::PI * length * length)
        * (210e3 * i / (7.85e-9 * area)).sqrt()
}

/// The hex of incompatible modes with the mass of the corners only gives the first frequency of
/// a cantilever within the budget of the validation case `grid-frequency-cantilever`: 0.3 % of
/// shear and rotary inertia and 0.7 % of the bending error of the grid.
#[test]
fn the_cantilever_on_the_grid_has_the_frequency_of_beam_theory() {
    let (s, modes) = run(&beam([100.0, 10.0, 5.0], 2.5, true, 3), None);
    let f1 = cantilever_f1(100.0, 10.0 * 125.0 / 12.0, 50.0);
    let err = modes[0].frequency / f1 - 1.0;
    assert!(err.abs() < 0.02, "f1 {} against {f1}", modes[0].frequency);
    // Mode 2 bends about the stiffer axis: twice the frequency, since h is twice.
    let f2 = cantilever_f1(100.0, 5.0 * 1000.0 / 12.0, 50.0);
    assert!((modes[1].frequency / f2 - 1.0).abs() < 0.02);
    // Mode 1 moves in z: its effective mass is mostly along z.
    assert!(modes[0].mass_fraction[2] > 0.5 && modes[0].mass_fraction[0] < 1e-6);
    assert!((s.mass - 7.85e-9 * 5000.0).abs() < 1e-15);
}

/// A free box has 6 rigid modes at 0, the shift keeps the factor positive definite, and the
/// effective masses of the rigid modes sum to the mass along each axis.
#[test]
fn a_free_box_has_6_rigid_modes() {
    for method in [Method::Direct, Method::Multigrid] {
        let (_, modes) = run(&beam([40.0, 30.0, 20.0], 4.0, false, 9), Some(method));
        let elastic = modes[6].eigenvalue;
        for m in &modes[..6] {
            assert!(
                m.eigenvalue.abs() < 1e-6 * elastic,
                "{method:?}: rigid {} against {elastic}",
                m.eigenvalue
            );
        }
        for d in 0..3 {
            let sum: f64 = modes[..6].iter().map(|m| m.mass_fraction[d]).sum();
            assert!((sum - 1.0).abs() < 1e-6, "{method:?}: {sum}");
        }
        assert!(modes[6].frequency > 1e3);
    }
}

/// The direct solver and multigrid give the same frequencies.
#[test]
fn the_2_solvers_give_the_same_frequencies() {
    let model = beam([60.0, 10.0, 10.0], 2.5, true, 4);
    let (_, direct) = run(&model, Some(Method::Direct));
    let (s, mg) = run(&model, Some(Method::Multigrid));
    assert_eq!(s.method, Method::Multigrid);
    for (a, b) in direct.iter().zip(&mg) {
        assert!((a.frequency / b.frequency - 1.0).abs() < 1e-8);
    }
}

/// A square section has 2 equal bending frequencies, and the subspace finds both.
#[test]
fn a_square_section_has_a_repeated_frequency() {
    let (_, modes) = run(&beam([80.0, 8.0, 8.0], 2.0, true, 2), None);
    let (a, b) = (modes[0].frequency, modes[1].frequency);
    assert!((a / b - 1.0).abs() < 1e-9, "{a} {b}");
    let f1 = cantilever_f1(80.0, 8f64.powi(4) / 12.0, 64.0);
    assert!((a / f1 - 1.0).abs() < 0.02);
}

/// A cantilever of square section as a polygon prism along x, on the 10-node tetrahedra, against
/// beam theory. A circle has at least 64 segments, so a round bar is too slow for a test.
#[test]
fn the_cantilever_on_tetrahedra_has_the_frequency_of_beam_theory() {
    let mut model = beam([100.0, 0.0, 0.0], 5.0, true, 2);
    let s = &mut model.structure;
    s.axis = 0;
    s.parts[0].hi = [100.0, 0.0, 0.0];
    s.parts[0].footprint = Some(Footprint {
        outline: Outline::Polygon(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]),
        holes: Vec::new(),
    });
    let (s, modes) = run(&model, None);
    assert_eq!(s.method, Method::Direct);
    let f1 = cantilever_f1(100.0, 1e4 / 12.0, 100.0);
    for m in &modes {
        assert!(
            (m.frequency / f1 - 1.0).abs() < 0.02,
            "{} {f1}",
            m.frequency
        );
    }
}

/// Each input outside the class of the modes solver gives an error.
#[test]
fn the_modes_solver_refuses_what_it_does_not_solve() {
    type Change = fn(&mut Model);
    let cases: [(Change, &str); 5] = [
        (|m| m.count = 0, "count"),
        (
            |m| m.structure.symmetry = [true, false, false],
            "whole model",
        ),
        (
            |m| m.structure.loads.push(Load::Gravity([0.0, 0.0, -9.81e3])),
            "only supports",
        ),
        (
            |m| {
                m.structure.loads[0] = Load::Displacement {
                    faces: 0,
                    value: [Some(0.1), None, None],
                }
            },
            "only supports",
        ),
        (|m| m.structure.materials[0].density = 0.0, "density"),
    ];
    for (change, text) in cases {
        let mut model = beam([20.0, 10.0, 10.0], 5.0, true, 2);
        change(&mut model);
        let e = solve(&model, &mut Record::default()).unwrap_err();
        assert!(e.contains(text), "{e}");
    }
}

/// Many modes of a large model need too much memory for the subspace vectors.
#[test]
fn the_modes_solver_refuses_subspace_vectors_above_2_gb() {
    assert!(eigen::fits(900_000, 6).is_ok());
    let e = eigen::fits(900_000, 100).unwrap_err();
    for text in [
        "100 modes",
        "900000 unknowns",
        "4.3 GB",
        "fewer modes",
        "CalculiX",
    ] {
        assert!(e.contains(text), "{e}");
    }
}
