//! Linear elasticity on the box grid: bending of thin parts against beam and plate theory, exact
//! uniform fields, the force balance, symmetry, the 2 linear solvers and the refusal of a model
//! with no unique solution.

use fh_fem::elastic::{
    solve, solve_within, Events, Load, Material, Model, Singular, Solution, Summary, Surface,
    Thermal,
};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{FaceSet, Mesh, Probe};
use fh_fem::hex::Orthotropic;
use fh_fem::prism::{Footprint, Outline};
use fh_fem::solve::{Method, PCG_MAX_ITER};

#[derive(Default)]
struct Record {
    mesh: Option<Mesh>,
    solution: Option<Solution>,
}

impl Events for Record {
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.mesh = Some(mesh.clone());
        Ok(())
    }
    fn progress(&mut self, text: &str) {
        if std::env::var_os("FH_VERBOSE").is_some() {
            eprintln!("{text}");
        }
    }
    fn solution(&mut self, s: &Solution) -> Result<(), String> {
        self.solution = Some(s.clone());
        Ok(())
    }
}

fn run(model: &Model) -> (Summary, Solution) {
    let mut r = Record::default();
    let s = solve(model, &mut r).unwrap();
    (s, r.solution.unwrap())
}

fn material(name: &str, e: f64, nu: f64, alpha: f64) -> Material {
    Material {
        name: name.into(),
        elastic: Orthotropic::isotropic(e, nu),
        expansion: [alpha; 3],
        density: 1.0,
    }
}

fn part(name: &str, material: usize, lo: [f64; 3], hi: [f64; 3]) -> Part {
    Part {
        name: name.into(),
        material,
        priority: 0,
        lo,
        hi,
        footprint: None,
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

fn probe(name: &str, point: [f64; 3]) -> Probe {
    Probe {
        name: name.into(),
        point,
    }
}

fn fix(faces: usize, x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Load {
    Load::Displacement {
        faces,
        value: [x, y, z],
    }
}

/// A cantilever along x, length l, width b, height h, clamped at x = 0, with a shear force P
/// down at x = l. nu = 0, so the clamp adds no stiffness across the width.
fn cantilever(l: f64, b: f64, h: f64, max_cell: f64, min_cells: usize, p: f64) -> Model {
    Model {
        parts: vec![part("beam", 0, [0.0; 3], [l, b, h])],
        materials: vec![material("steel", 200e3, 0.0, 0.0)],
        spacing: Spacing {
            max_cell,
            max_growth: 1.3,
            min_cells,
        },
        symmetry: [false; 3],
        faces: vec![side("root", "x-"), side("tip", "x+")],
        loads: vec![
            fix(0, Some(0.0), Some(0.0), Some(0.0)),
            Load::Traction {
                faces: 1,
                value: [0.0, 0.0, -p / (b * h)],
            },
        ],
        thermal: Thermal::None,
        reference: 0.0,
        probes: vec![probe("tip", [l, b / 2.0, h / 2.0])],
        surfaces: vec![],
        method: None,
    }
}

/// Euler-Bernoulli plus the shear deflection of Timoshenko (shear factor 5/6).
fn beam_deflection(l: f64, b: f64, h: f64, e: f64, nu: f64, p: f64) -> f64 {
    let i = b * h.powi(3) / 12.0;
    let g = e / (2.0 * (1.0 + nu));
    p * l.powi(3) / (3.0 * e * i) + p * l / (5.0 / 6.0 * g * b * h)
}

/// The tip deflection of a slender cantilever with 1 and 2 cells through the height is within
/// 1.5 % of beam theory. A plain 8-node hex gives 0.35 to 2.07 times the beam deflection
/// (DESIGN.md), so this is the check that the element does not lock.
#[test]
fn a_cantilever_bends_as_a_beam_with_few_cells_through_its_height() {
    let (l, b, h, p) = (100.0, 10.0, 5.0, 100.0);
    let exact = beam_deflection(l, b, h, 200e3, 0.0, p);
    for (max_cell, min_cells) in [(5.0, 1), (2.5, 2), (1.25, 4)] {
        let (summary, s) = run(&cantilever(l, b, h, max_cell, min_cells, p));
        let w = -s.probes[0].displacement[2];
        let ratio = w / exact;
        assert!((ratio - 1.0).abs() < 0.015, "max_cell {max_cell}: {ratio}");
        assert!(summary.imbalance < 1e-9, "{}", summary.imbalance);
        // The reaction holds the load.
        assert!((s.reactions[0].force[2] - p).abs() < 1e-6 * p);
    }
}

/// A clamped square plate, side a and thickness t with a / t = 100, under a uniform pressure q:
/// the centre deflection of the Kirchhoff plate is 0.00126 q a^4 / D (Timoshenko and
/// Woinowsky-Krieger, Theory of Plates and Shells, 1959, table 35). The cells are 10 to 40 times
/// longer than they are thick.
#[test]
fn a_thin_clamped_plate_bends_as_a_kirchhoff_plate() {
    let (a, t, q, e, nu): (f64, f64, f64, f64, f64) = (100.0, 1.0, 1e-3, 200e3, 0.3);
    let d = e * t.powi(3) / (12.0 * (1.0 - nu * nu));
    let exact = 0.00126 * q * a.powi(4) / d;
    let mut last = 0.0;
    for max_cell in [10.0, 5.0, 2.5] {
        let model = Model {
            parts: vec![part("plate", 0, [0.0; 3], [a, a, t])],
            materials: vec![material("steel", e, nu, 0.0)],
            spacing: Spacing {
                max_cell,
                max_growth: 1.3,
                min_cells: 2,
            },
            symmetry: [true, true, false],
            faces: vec![
                FaceSet {
                    name: "edges".into(),
                    sides: ["x-", "x+", "y-", "y+"]
                        .map(|s| Side::parse(s).unwrap())
                        .to_vec(),
                    region: None,
                    parts: vec![],
                    circle: None,
                },
                side("top", "z+"),
            ],
            loads: vec![
                fix(0, Some(0.0), Some(0.0), Some(0.0)),
                Load::Pressure { faces: 1, value: q },
            ],
            thermal: Thermal::None,
            reference: 0.0,
            probes: vec![probe("centre", [a / 2.0, a / 2.0, t / 2.0])],
            surfaces: vec![],
            method: None,
        };
        let (_, s) = run(&model);
        last = -s.probes[0].displacement[2] / exact;
        eprintln!("max_cell {max_cell}: w / w_kirchhoff = {last:.5}");
    }
    assert!((last - 1.0).abs() < 0.01, "{last}");
}

/// A free bar with a uniform temperature change and a statically determinate support expands
/// by alpha dT with no stress: the exact field is linear, so the error is round-off.
#[test]
fn a_free_uniform_expansion_has_no_stress() {
    let (alpha, dt) = (1e-5, 100.0);
    let model = Model {
        parts: vec![part("bar", 0, [0.0; 3], [4.0, 2.0, 1.0])],
        materials: vec![material("m", 100e3, 0.3, alpha)],
        spacing: Spacing {
            max_cell: 0.5,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("x0", "x-"), side("y0", "y-"), side("z0", "z-")],
        loads: vec![
            fix(0, Some(0.0), None, None),
            fix(1, None, Some(0.0), None),
            fix(2, None, None, Some(0.0)),
        ],
        thermal: Thermal::Uniform(dt + 20.0),
        reference: 20.0,
        probes: vec![probe("far", [4.0, 2.0, 1.0]), probe("mid", [1.3, 0.7, 0.2])],
        surfaces: vec![],
        method: Some(Method::Direct),
    };
    let (_, s) = run(&model);
    let far = s.probes[0].displacement;
    for d in 0..3 {
        let want = alpha * dt * [4.0, 2.0, 1.0][d];
        assert!((far[d] - want).abs() < 1e-12, "{far:?}");
    }
    assert!(s.mises.iter().all(|v| v.abs() < 1e-9), "{:?}", s.peaks);
    assert!(s.probes[1].mises < 1e-9);
}

/// 3 point supports that hold only the 6 rigid motions leave a free expansion free. A point
/// support that takes a force gives a warning, and a point that is not on the grid is refused.
#[test]
fn point_supports_hold_only_the_rigid_motions() {
    let (alpha, dt) = (1e-5, 100.0);
    let point = |x: [f64; 3], v: [Option<f64>; 3]| Load::Point { point: x, value: v };
    let zero = Some(0.0);
    let mut model = Model {
        parts: vec![part("bar", 0, [0.0; 3], [4.0, 2.0, 1.0])],
        materials: vec![material("m", 100e3, 0.3, alpha)],
        spacing: Spacing {
            max_cell: 0.5,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![],
        loads: vec![
            point([0.0; 3], [zero, zero, zero]),
            point([4.0, 0.0, 0.0], [None, zero, zero]),
            point([0.0, 2.0, 0.0], [None, None, zero]),
        ],
        thermal: Thermal::Uniform(dt),
        reference: 0.0,
        probes: vec![probe("far", [4.0, 2.0, 1.0])],
        surfaces: vec![],
        method: None,
    };
    let (summary, s) = run(&model);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert!(summary.imbalance < 1e-12, "{}", summary.imbalance);
    let far = s.probes[0].displacement;
    for d in 0..3 {
        let want = alpha * dt * [4.0, 2.0, 1.0][d];
        assert!((far[d] - want).abs() < 1e-12, "{far:?}");
    }
    assert!(s.mises.iter().all(|v| v.abs() < 1e-9));

    // A 4th point that holds uz of a top corner takes a force: the expansion lifts that corner.
    model.loads.push(point([4.0, 2.0, 1.0], [None, None, zero]));
    let (summary, _) = run(&model);
    assert!(
        summary.warnings.iter().any(|w| w.contains("point support")),
        "{:?}",
        summary.warnings
    );

    model.loads[3] = point([1.1, 0.0, 0.0], [None, None, zero]);
    let e = solve(&model, &mut Record::default()).unwrap_err();
    assert!(e.contains("not a point of the grid"), "{e}");
}

/// The direct solver and multigrid PCG give the same displacement on a stiff block on a soft
/// layer with gravity.
#[test]
fn the_direct_and_the_multigrid_solvers_agree() {
    let model = |method| Model {
        parts: vec![
            part("base", 0, [0.0; 3], [10.0, 6.0, 1.0]),
            part("top", 1, [3.0, 1.0, 1.0], [7.0, 5.0, 3.0]),
        ],
        materials: vec![
            material("soft", 1e3, 0.45, 0.0),
            material("stiff", 2e5, 0.3, 0.0),
        ],
        spacing: Spacing {
            max_cell: 0.4,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("bottom", "z-")],
        loads: vec![
            fix(0, Some(0.0), Some(0.0), Some(0.0)),
            Load::Gravity([1.0, 0.0, -9.81]),
        ],
        thermal: Thermal::None,
        reference: 0.0,
        probes: vec![probe("top", [5.0, 3.0, 3.0])],
        surfaces: vec![],
        method: Some(method),
    };
    let (sd, d) = run(&model(Method::Direct));
    let (sm, m) = run(&model(Method::Multigrid));
    assert!(sd.unknowns > 5000, "{}", sd.unknowns);
    let scale = d
        .displacement
        .iter()
        .flatten()
        .fold(0.0f64, |a, v| a.max(v.abs()));
    for (a, b) in d.displacement.iter().zip(&m.displacement) {
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() < 1e-7 * scale);
        }
    }
    assert!(sd.imbalance < 1e-9 && sm.imbalance < 1e-7);
}

/// A quarter model with 2 planes of symmetry gives the values of the full model, and the
/// surface shape counts the mirror faces.
#[test]
fn a_quarter_model_gives_the_values_of_the_full_model() {
    let model = |symmetry| Model {
        parts: vec![
            part("plate", 0, [0.0; 3], [8.0, 8.0, 0.5]),
            part("die", 1, [2.0, 2.0, 0.5], [6.0, 6.0, 0.8]),
        ],
        materials: vec![
            material("a", 100e3, 0.3, 2e-5),
            material("b", 150e3, 0.25, 3e-6),
        ],
        spacing: Spacing {
            max_cell: 0.5,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry,
        faces: vec![
            FaceSet {
                name: "centre".into(),
                sides: vec![Side::parse("z-").unwrap()],
                region: Some(([3.5, 3.5, 0.0], [4.5, 4.5, 0.0])),
                parts: vec![],
                circle: None,
            },
            side("bottom", "z-"),
        ],
        loads: vec![fix(0, None, None, Some(0.0))],
        thermal: Thermal::Uniform(-100.0),
        reference: 0.0,
        probes: vec![
            probe("corner", [0.0, 0.0, 0.0]),
            probe("edge", [7.9, 1.1, 0.45]),
        ],
        surfaces: vec![Surface {
            name: "bottom".into(),
            faces: 1,
        }],
        method: Some(Method::Direct),
    };
    // The full model needs the in-plane motion held: fix the centre faces in x and y too.
    let mut full = model([false; 3]);
    full.loads = vec![fix(0, Some(0.0), Some(0.0), Some(0.0))];
    // The full model's centre clamp is a support edge, so the quarter one gets the same faces.
    let mut quarter = model([true, true, false]);
    quarter.loads = vec![fix(0, Some(0.0), Some(0.0), Some(0.0))];
    let (_, f) = run(&full);
    let (_, q) = run(&quarter);
    for k in 0..2 {
        let (a, b) = (f.probes[k], q.probes[k]);
        for d in 0..3 {
            assert!(
                (a.displacement[d] - b.displacement[d]).abs() < 1e-9,
                "{k}: {a:?} {b:?}"
            );
        }
        for c in 0..6 {
            assert!((a.stress[c] - b.stress[c]).abs() < 1e-6, "{k}: {a:?} {b:?}");
        }
    }
    let (a, b) = (&f.surfaces[0], &q.surfaces[0]);
    assert!((a.range - b.range).abs() < 1e-9 && (a.flatness - b.flatness).abs() < 1e-9);
    for k in 0..3 {
        assert!((a.curvature[k] - b.curvature[k]).abs() < 1e-9 * a.curvature[0].abs());
    }
    // The plate shrinks more than the die, so the plate is on the concave side of the bend.
    assert_eq!(a.sign(), "concave", "{a:?}");
}

/// A part that can still move or turn is refused with a message that names the motion.
#[test]
fn a_model_that_can_move_is_refused() {
    let mut m = cantilever(10.0, 1.0, 1.0, 0.5, 2, 1.0);
    m.loads[0] = fix(0, Some(0.0), None, Some(0.0));
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("move along y"), "{e}");
}

/// The static step solves only on the box grid, so a prism part is refused, not solved as a box.
#[test]
fn a_prism_part_is_refused() {
    let mut m = cantilever(10.0, 1.0, 1.0, 0.5, 2, 1.0);
    m.parts[0].footprint = Some(Footprint {
        outline: Outline::Circle {
            centre: [0.5, 0.5],
            radius: 0.5,
        },
        holes: vec![],
    });
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("parts that are boxes"), "{e}");
}

/// 2 parts of 1 material are 1 solid: the line where they meet on a free face is not an edge
/// where 2 materials meet.
#[test]
fn two_parts_of_one_material_have_no_material_edge() {
    let mut m = cantilever(20.0, 2.0, 2.0, 1.0, 2, 100.0);
    m.parts = vec![
        part("root", 0, [0.0; 3], [10.0, 2.0, 2.0]),
        part("end", 0, [10.0, 0.0, 0.0], [20.0, 2.0, 2.0]),
    ];
    let (_, s) = run(&m);
    assert_eq!(s.peaks[0].singular, Some(Singular::SupportEdge));
    assert_eq!(s.peaks[1].singular, None, "{:?}", s.peaks[1]);
}

/// Above the ceiling of the factor entries, a model with flat cells gets multigrid and the
/// result of the direct solver. When multigrid does not converge on the flat cells, the error
/// names the thin part and the remedy.
#[test]
fn a_factor_above_the_ceiling_gives_multigrid() {
    let m = cantilever(20.0, 10.0, 1.0, 2.0, 2, 1.0);
    let (sd, d) = run(&m);
    let mut r = Record::default();
    let sm = solve_within(&m, 0, PCG_MAX_ITER, &mut r).unwrap();
    let mg = r.solution.unwrap();
    assert_eq!((sd.method, sm.method), (Method::Direct, Method::Multigrid));
    let (wd, wm) = (d.probes[0].displacement[2], mg.probes[0].displacement[2]);
    assert!((wd - wm).abs() < 1e-6 * wd.abs(), "{wd} {wm}");

    let e = solve_within(&m, 0, 1, &mut Record::default()).unwrap_err();
    assert!(e.contains("did not converge"), "{e}");
    assert!(e.contains("The cells of part beam are 4 times"), "{e}");
    assert!(
        e.contains("a smaller max_cell or a larger min_cells"),
        "{e}"
    );
}

/// A stiffened panel: a skin 0.002 thick in z with 4 ribs 0.003 thick in y, clamped at x = 0
/// under a pressure on the skin. Its thin parts are thin along 2 axes.
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
        materials: vec![material("aluminium", 70e9, 0.33, 0.0)],
        spacing: Spacing {
            max_cell: 1.0 / n as f64,
            max_growth: 1.3,
            min_cells: 2,
        },
        symmetry: [false; 3],
        faces: vec![side("end", "x-"), side("skin", "z-")],
        loads: vec![
            fix(0, Some(0.0), Some(0.0), Some(0.0)),
            Load::Pressure {
                faces: 1,
                value: 1e4,
            },
        ],
        thermal: Thermal::None,
        reference: 0.0,
        probes: vec![],
        surfaces: vec![],
        method: None,
    }
}

/// Times of the direct solver and of PCG with multigrid, for `DIRECT_FLOPS`, `MULTIGRID_ASPECT`
/// and `MAX_POINTS`: cubes of n^3 cells clamped at the bottom under gravity, plates of 2 cell
/// layers and the panel. Run with
/// `cargo test --release -p fh-fem --test elastic crossover -- --ignored --nocapture`.
#[test]
#[ignore]
fn crossover() {
    let cube = |n: usize| {
        let mut m = cantilever(1.0, 1.0, 1.0, 1.0 / n as f64, 2, 0.0);
        m.loads = vec![
            fix(0, Some(0.0), Some(0.0), Some(0.0)),
            Load::Gravity([0.0, 1.0, -9.81]),
        ];
        m.probes.clear();
        m
    };
    let time = |name: &str, m: &mut Model, direct: bool| {
        let mut out = Vec::new();
        for method in [Method::Direct, Method::Multigrid] {
            if method == Method::Direct && !direct {
                out.push((0, f64::NAN));
                continue;
            }
            m.method = Some(method);
            let start = std::time::Instant::now();
            match solve(m, &mut Record::default()) {
                Ok(s) => out.push((s.unknowns, start.elapsed().as_secs_f64())),
                Err(e) => {
                    println!("{name} {method:?}: {e}");
                    out.push((0, f64::NAN));
                }
            }
        }
        println!(
            "{name} unknowns {} direct {:.3} s multigrid {:.3} s",
            out[1].0, out[0].1, out[1].1
        );
    };
    for n in [6, 10, 14, 18, 22, 26, 30] {
        time(&format!("cube n {n}"), &mut cube(n), true);
    }
    for t in [1.0, 0.25] {
        for cell in [2.0, 1.0, 0.5] {
            let mut m = cantilever(100.0, 100.0, t, cell, 2, 0.0);
            m.loads = vec![
                fix(0, Some(0.0), Some(0.0), Some(0.0)),
                Load::Gravity([0.0, 0.0, -9.81]),
            ];
            m.probes.clear();
            time(&format!("plate t {t} cell {cell}"), &mut m, true);
        }
    }
    for n in [10, 20] {
        time(&format!("panel n {n}"), &mut panel(n), true);
    }
}
