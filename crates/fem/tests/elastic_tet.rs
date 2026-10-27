//! Linear elasticity on the extruded mesh of 10-node tetrahedra: exact uniform fields, a free
//! thermal expansion, the 2 linear solvers, the singular points and the refusal of a model with
//! no unique solution. The validation cases (grid-kirsch-hole, grid-lame-cylinder,
//! grid-round-cantilever, grid-plate-hole-stress-ccx) check the accuracy against analytic
//! answers and CalculiX.

use fh_fem::elastic::{solve, Events, Load, Material, Model, Singular, Solution, Summary, Thermal};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{Cell, FaceSet, Mesh, Probe};
use fh_fem::hex::Orthotropic;
use fh_fem::prism::{Footprint, Outline};
use fh_fem::solve::Method;

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

fn run(model: &Model) -> (Summary, Mesh, Solution) {
    let mut r = Record::default();
    let s = solve(model, &mut r).unwrap();
    (s, r.mesh.unwrap(), r.solution.unwrap())
}

fn material(e: f64, nu: f64, alpha: f64) -> Material {
    Material {
        name: "m".into(),
        elastic: Orthotropic::isotropic(e, nu),
        expansion: [alpha; 3],
        density: 0.0,
    }
}

/// A prism along z of `outline` with `holes`, from z0 to z1.
fn prism(name: &str, material: usize, outline: Outline, holes: Vec<Outline>, z: [f64; 2]) -> Part {
    Part {
        name: name.into(),
        material,
        priority: 0,
        lo: [0.0, 0.0, z[0]],
        hi: [0.0, 0.0, z[1]],
        footprint: Some(Footprint { outline, holes }),
    }
}

fn circle(r: f64) -> Outline {
    Outline::Circle {
        centre: [0.0, 0.0],
        radius: r,
    }
}

fn faces(name: &str, sides: &[&str]) -> FaceSet {
    FaceSet {
        name: name.into(),
        sides: sides.iter().map(|s| Side::parse(s).unwrap()).collect(),
        region: None,
        parts: vec![],
        circle: None,
    }
}

fn fix(faces: usize, x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Load {
    Load::Displacement {
        faces,
        value: [x, y, z],
    }
}

fn point(at: [f64; 3], x: Option<f64>, y: Option<f64>) -> Load {
    Load::Point {
        point: at,
        value: [x, y, None],
    }
}

fn model(parts: Vec<Part>, materials: Vec<Material>, max_cell: f64) -> Model {
    Model {
        parts,
        materials,
        spacing: Spacing {
            max_cell,
            max_growth: 1.3,
            min_cells: 2,
        },
        axis: 2,
        symmetry: [false; 3],
        faces: vec![],
        loads: vec![],
        thermal: Thermal::None,
        reference: 0.0,
        probes: vec![],
        surfaces: vec![],
        method: None,
    }
}

/// A ring r < 1 with a hole r < 0.5, z 0 to 0.4, held so that it can expand freely: uz on the
/// bottom, and the points at 0, 90 and 180 degrees on the outer circle stop the motions in the
/// plane.
fn ring(max_cell: f64) -> Model {
    let mut m = model(
        vec![prism("ring", 0, circle(1.0), vec![circle(0.5)], [0.0, 0.4])],
        vec![material(200e3, 0.3, 1e-5)],
        max_cell,
    );
    m.faces = vec![faces("bottom", &["z-"])];
    m.loads = vec![
        fix(0, None, None, Some(0.0)),
        point([1.0, 0.0, 0.0], None, Some(0.0)),
        point([-1.0, 0.0, 0.0], None, Some(0.0)),
        point([0.0, 1.0, 0.0], Some(0.0), None),
    ];
    m
}

/// A uniform stress is exact: a prism of a pentagon with a round hole, held by rollers on x-, y-
/// and z-, with a traction on z+. Each side face is along z, so it is free under szz = 10. Each
/// point has szz = 10 and the displacement of the uniform strain, with an orthotropic material.
#[test]
fn a_uniform_stress_is_exact() {
    let pentagon = Outline::Polygon(vec![
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [1.0, 1.6],
        [0.0, 1.0],
    ]);
    let hole = Outline::Circle {
        centre: [1.0, 0.6],
        radius: 0.3,
    };
    let mut m = model(
        vec![prism("p", 0, pentagon, vec![hole], [0.0, 0.5])],
        vec![],
        0.3,
    );
    let elastic = Orthotropic {
        e: [100e3, 50e3, 20e3],
        nu: [0.3, 0.2, 0.1],
        g: [30e3, 10e3, 8e3],
    };
    m.materials = vec![Material {
        elastic,
        ..material(0.0, 0.0, 0.0)
    }];
    m.faces = vec![
        faces("x-", &["x-"]),
        faces("y-", &["y-"]),
        faces("z-", &["z-"]),
        faces("z+", &["z+"]),
    ];
    m.loads = vec![
        fix(0, Some(0.0), None, None),
        fix(1, None, Some(0.0), None),
        fix(2, None, None, Some(0.0)),
        Load::Traction {
            faces: 3,
            value: [0.0, 0.0, 10.0],
        },
    ];
    m.probes = vec![Probe {
        name: "inside".into(),
        point: [1.7, 0.3, 0.2],
    }];
    let (summary, mesh, s) = run(&m);
    assert_eq!(mesh.kind, Cell::Tet10);
    assert!(summary.imbalance < 1e-9, "{summary:?}");
    // The strain of szz = 10: -nu_xz 10 / Ex along x, -nu_yz 10 / Ey along y, 10 / Ez along z.
    let e = [-0.2 * 10.0 / 100e3, -0.1 * 10.0 / 50e3, 10.0 / 20e3];
    for (p, (u, sig)) in mesh.points.iter().zip(s.displacement.iter().zip(&s.stress)) {
        for d in 0..3 {
            assert!((u[d] - e[d] * p[d]).abs() < 1e-12, "{p:?} {u:?}");
        }
        assert!((sig[2] - 10.0).abs() < 1e-8, "{p:?} {sig:?}");
        for (k, v) in sig.iter().enumerate() {
            assert!(k == 2 || v.abs() < 1e-8, "{p:?} {sig:?}");
        }
    }
    assert!((s.probes[0].stress[2] - 10.0).abs() < 1e-8);
    // The roller on z- takes the force of the traction on z+: the pentagon of area 2.6 minus the
    // hole, a polygon of 64 segments.
    let n = 64.0;
    let area = 2.6 - 0.5 * n * 0.3f64.powi(2) * (2.0 * std::f64::consts::PI / n).sin();
    assert!(
        (s.reactions[2].force[2] + 10.0 * area).abs() < 1e-8,
        "{:?}",
        s.reactions
    );
}

/// A uniform temperature change on a free ring gives no stress and the displacement
/// alpha dT x from the centre, with the point supports on the circle.
#[test]
fn a_free_ring_expands_with_no_stress() {
    let mut m = ring(0.25);
    m.thermal = Thermal::Uniform(120.0);
    m.reference = 20.0;
    let (summary, mesh, s) = run(&m);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    let strain = 1e-5 * 100.0;
    for (p, u) in mesh.points.iter().zip(&s.displacement) {
        for d in 0..3 {
            assert!((u[d] - strain * p[d]).abs() < 1e-12, "{p:?} {u:?}");
        }
    }
    // E alpha dT is 200: the stress is 0 to the precision of the solve.
    let peak = s.mises.iter().fold(0.0f64, |a, &b| a.max(b));
    assert!(peak < 1e-6, "{peak}");
    assert_eq!(
        s.temperature.as_ref().map(Vec::len),
        Some(mesh.points.len())
    );
}

/// The direct solver and PCG with p-multigrid give the same displacement, for a ring under
/// pressure on its hole.
#[test]
fn the_direct_and_the_p_multigrid_solvers_agree() {
    let mut m = ring(0.2);
    m.faces.push(FaceSet {
        circle: Some(([0.0, 0.0], 0.5)),
        ..faces("hole", &[])
    });
    m.loads.push(Load::Pressure {
        faces: 1,
        value: 50.0,
    });
    let solve_with = |method| {
        let mut m = m.clone();
        m.method = Some(method);
        run(&m)
    };
    let (direct, _, a) = solve_with(Method::Direct);
    let (iterative, _, b) = solve_with(Method::Multigrid);
    assert_eq!(direct.method, Method::Direct);
    assert_eq!(iterative.method, Method::Multigrid);
    let scale = a
        .displacement
        .iter()
        .flatten()
        .fold(0.0f64, |m, v| m.max(v.abs()));
    for (u, v) in a.displacement.iter().zip(&b.displacement) {
        for d in 0..3 {
            assert!((u[d] - v[d]).abs() < 1e-8 * scale, "{u:?} {v:?}");
        }
    }
}

/// The singular points: the inner corner of an L is re-entrant, the edge where a disk of 1
/// material meets a plate of another on a free face is a material edge, and the edge of a hole
/// on a free face is regular.
#[test]
fn the_singular_points_are_the_kinks_of_the_solid() {
    // An L clamped on x- with a traction on its far end: the peak is at the inner corner.
    let l = Outline::Polygon(vec![
        [0.0, 0.0],
        [2.0, 0.0],
        [2.0, 1.0],
        [1.0, 1.0],
        [1.0, 2.0],
        [0.0, 2.0],
    ]);
    let mut m = model(
        vec![prism("l", 0, l, vec![], [0.0, 0.3])],
        vec![material(200e3, 0.3, 0.0)],
        0.2,
    );
    m.faces = vec![faces("top", &["y+"]), faces("end", &["x+"])];
    m.faces[0].region = Some(([0.0, 2.0, 0.0], [1.0, 2.0, 0.3]));
    m.loads = vec![
        fix(0, Some(0.0), Some(0.0), Some(0.0)),
        Load::Traction {
            faces: 1,
            value: [0.0, 10.0, 0.0],
        },
    ];
    let (_, _, s) = run(&m);
    assert_eq!(
        s.peaks[0].singular,
        Some(Singular::ReEntrant),
        "{:?}",
        s.peaks
    );
    let x = s.peaks[0].point;
    assert!(
        (x[0] - 1.0).abs() < 1e-9 && (x[1] - 1.0).abs() < 1e-9,
        "{x:?}"
    );

    // A plate with a hole under tension: the peak is at the hole, and it is regular.
    let square = Outline::Polygon(vec![[-4.0, -4.0], [4.0, -4.0], [4.0, 4.0], [-4.0, 4.0]]);
    let mut m = model(
        vec![prism("plate", 0, square, vec![circle(1.0)], [0.0, 0.5])],
        vec![material(200e3, 0.0, 0.0)],
        1.0,
    );
    m.faces = vec![
        faces("x-", &["x-"]),
        faces("x+", &["x+"]),
        faces("z-", &["z-"]),
    ];
    m.loads = vec![
        fix(0, Some(0.0), None, None),
        fix(2, None, None, Some(0.0)),
        Load::Traction {
            faces: 1,
            value: [10.0, 0.0, 0.0],
        },
        Load::Point {
            point: [-4.0, -4.0, 0.0],
            value: [None, Some(0.0), None],
        },
    ];
    let (_, _, s) = run(&m);
    let peak = &s.peaks[0];
    assert_eq!(peak.singular, None, "{peak:?}");
    assert!(peak.point[0].hypot(peak.point[1]) < 1.0 + 1e-9, "{peak:?}");

    // A disk of a stiff material bonded on top of a plate: the circle where they meet on the
    // top face of the plate is a material edge.
    let square = Outline::Polygon(vec![[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]]);
    let mut m = model(
        vec![
            prism("plate", 0, square, vec![], [0.0, 0.5]),
            prism("disk", 1, circle(1.0), vec![], [0.5, 1.0]),
        ],
        vec![material(100e3, 0.3, 2e-5), material(300e3, 0.3, 0.0)],
        0.4,
    );
    m.faces = vec![faces("bottom", &["z-"])];
    m.loads = vec![
        fix(0, None, None, Some(0.0)),
        point([2.0, 2.0, 0.0], Some(0.0), Some(0.0)),
        point([-2.0, 2.0, 0.0], None, Some(0.0)),
    ];
    m.thermal = Thermal::Uniform(100.0);
    let (summary, _, s) = run(&m);
    assert!(
        s.peaks
            .iter()
            .any(|p| p.singular == Some(Singular::MaterialEdge)),
        "{:?}",
        s.peaks
    );
    assert!(
        summary
            .warnings
            .iter()
            .any(|w| w.contains("an edge where 2 materials meet")),
        "{:?}",
        summary.warnings
    );
}

/// A ring with no support in its plane can turn about z, and a point support that is not a
/// corner of the mesh is refused, as is symmetry.
#[test]
fn a_model_with_no_unique_solution_is_refused() {
    let mut m = ring(0.25);
    m.thermal = Thermal::Uniform(1.0);
    m.loads.truncate(1);
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("move along x"), "{e}");
    m.loads.push(point([0.7, 0.0, 0.0], None, Some(0.0)));
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("not a corner of the mesh"), "{e}");
    let mut m = ring(0.25);
    m.symmetry = [true, false, false];
    let e = solve(&m, &mut Record::default()).unwrap_err();
    assert!(e.contains("symmetry needs a model of boxes"), "{e}");
}

/// The time of the direct solver and of PCG with p-multigrid on a solid ring and on plates of
/// several thicknesses, for the choice of the solver. Run with `cargo test --release -p fh-fem
/// --test elastic_tet crossover -- --ignored --nocapture`.
#[test]
#[ignore]
fn crossover() {
    let time = |name: &str, m: &Model| {
        let mut out = Vec::new();
        for method in [Method::Direct, Method::Multigrid] {
            let mut m = m.clone();
            m.method = Some(method);
            let start = std::time::Instant::now();
            let mut r = Record::default();
            match solve(&m, &mut r) {
                Ok(s) => out.push((s.unknowns, start.elapsed().as_secs_f64())),
                Err(e) => {
                    println!("{name} {method:?}: {e}");
                    out.push((0, f64::NAN));
                }
            }
        }
        println!(
            "{name} unknowns {} direct {:.3} s p-multigrid {:.3} s",
            out[1].0, out[0].1, out[1].1
        );
    };
    for cell in [0.4, 0.25, 0.15, 0.1] {
        let mut m = ring(cell);
        m.parts[0].hi[2] = 1.0;
        m.thermal = Thermal::Uniform(1.0);
        m.faces.push(FaceSet {
            circle: Some(([0.0, 0.0], 0.5)),
            ..faces("hole", &[])
        });
        m.loads.push(Load::Pressure {
            faces: 1,
            value: 50.0,
        });
        time(&format!("ring cell {cell}"), &m);
    }
    for t in [1.0, 0.25, 0.1] {
        for cell in [2.0, 1.0] {
            let square = Outline::Polygon(vec![[0.0, 0.0], [50.0, 0.0], [50.0, 50.0], [0.0, 50.0]]);
            let mut m = model(
                vec![prism("plate", 0, square, vec![circle(5.0)], [0.0, t])],
                vec![material(200e3, 0.3, 0.0)],
                cell,
            );
            m.parts[0].footprint.as_mut().unwrap().holes = vec![Outline::Circle {
                centre: [25.0, 25.0],
                radius: 5.0,
            }];
            m.faces = vec![faces("x-", &["x-"])];
            m.loads = vec![
                fix(0, Some(0.0), Some(0.0), Some(0.0)),
                Load::Gravity([0.0, 0.0, -9.81]),
            ];
            m.materials[0].density = 7.85e-9;
            time(&format!("plate t {t} cell {cell}"), &m);
        }
    }
}
