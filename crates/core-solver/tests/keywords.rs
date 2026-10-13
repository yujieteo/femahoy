//! One model with every keyword of the M2 linear static subset, read and solved:
//! `fixtures/inp/m2-keywords.inp`. A bar of 3 unit cubes along x: C3D8I, C3D8 and 6 C3D10,
//! joined by `*MPC` and `*EQUATION`, clamped and pulled through `*COUPLING`, then pulled by
//! a `*DLOAD` pressure. Poisson's ratio is 0, so the exact answer is a uniform tension of 10.
//! The file header gives the details. Also the errors for a missing set, a missing material
//! and a bad element type.

use std::path::Path;

use fh_core_solver::solve;
use fh_inp::{parse, Boundary, Cload, CouplingKind, Dload, ElementTarget, Model, MpcKind, Target};

fn text() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/inp/m2-keywords.inp");
    std::fs::read_to_string(p).unwrap()
}

fn model() -> Model {
    parse(&text()).unwrap_or_else(|e| panic!("{e}"))
}

fn ids(m: &Model, set: &str) -> Vec<u32> {
    m.nsets.iter().find(|s| s.name == set).unwrap().ids.clone()
}

fn elset(m: &Model, set: &str) -> Vec<u32> {
    m.elsets.iter().find(|s| s.name == set).unwrap().ids.clone()
}

#[test]
fn every_keyword_of_the_model_is_read() {
    let m = model();
    // *NODE (two blocks) and *ELEMENT with three types.
    assert_eq!(m.nodes.len(), 41);
    let count = |ty: &str| m.elements.iter().filter(|e| e.element_type == ty).count();
    assert_eq!((count("C3D8I"), count("C3D8"), count("C3D10")), (1, 1, 6));
    assert!(m
        .elements
        .iter()
        .all(|e| e.nodes.len() == if e.element_type == "C3D10" { 10 } else { 8 }));
    // *NSET and *ELSET, with GENERATE and with ELSET= on *ELEMENT.
    assert_eq!(ids(&m, "START"), [1, 2, 3, 4]);
    assert_eq!(ids(&m, "END").len(), 9);
    assert_eq!(elset(&m, "EA"), [1]);
    assert_eq!(elset(&m, "EB"), [2]);
    assert_eq!(elset(&m, "EC"), [3, 4, 5, 6, 7, 8]);
    assert_eq!(elset(&m, "EC_LIST"), elset(&m, "EC"));
    // *MATERIAL, *ELASTIC and *SOLID SECTION.
    let e: Vec<(&str, f64, f64)> = m
        .materials
        .iter()
        .map(|x| {
            let el = x.elastic.as_ref().unwrap();
            (x.name.as_str(), el.youngs_modulus, el.poissons_ratio)
        })
        .collect();
    assert_eq!(e, [("SOFT", 1000.0, 0.0), ("STIFF", 2000.0, 0.0)]);
    let s: Vec<(&str, &str)> = m
        .sections
        .iter()
        .map(|x| (x.elset.as_str(), x.material.as_str()))
        .collect();
    assert_eq!(s, [("EA", "SOFT"), ("EB", "STIFF"), ("EC", "SOFT")]);
    // *SURFACE, TYPE=NODE and *COUPLING with *KINEMATIC and *DISTRIBUTING.
    assert_eq!(m.surfaces.len(), 2);
    assert_eq!(m.surfaces[0].ids, ids(&m, "START"));
    assert_eq!(m.surfaces[1].ids, ids(&m, "END"));
    let c: Vec<(&str, u32, &str, CouplingKind)> = m
        .couplings
        .iter()
        .map(|x| (x.name.as_str(), x.ref_node, x.surface.as_str(), x.kind))
        .collect();
    assert_eq!(
        c,
        [
            ("CLAMP", 100, "END_FACE", CouplingKind::Kinematic),
            ("PULL", 101, "START_FACE", CouplingKind::Distributing)
        ]
    );
    assert_eq!((m.couplings[1].first_dof, m.couplings[1].last_dof), (1, 3));
    // *EQUATION: 4 edge midsides (3 terms) and 1 face diagonal midside (5 terms), 3 dofs.
    assert_eq!(m.equations.len(), 15);
    let terms = |n: usize| m.equations.iter().filter(|q| q.terms.len() == n).count();
    assert_eq!((terms(3), terms(5)), (12, 3));
    for q in &m.equations {
        let sum: f64 = q.terms.iter().map(|t| t.coefficient).sum();
        assert!(sum.abs() < 1e-15, "{q:?}");
    }
    // *MPC: TIE, PIN, BEAM and LINK.
    let kinds = |k: MpcKind| m.mpcs.iter().filter(|x| x.kind == k).count();
    let (tie, pin) = (kinds(MpcKind::Tie), kinds(MpcKind::Pin));
    let (beam, link) = (kinds(MpcKind::Beam), kinds(MpcKind::Link));
    assert_eq!((tie, pin, beam, link), (2, 2, 2, 1));
    // *BOUNDARY in the model and in a step, *STEP, *STATIC, *CLOAD and *DLOAD.
    let fix = |first, last| Boundary {
        target: Target::Node(100),
        first_dof: first,
        last_dof: last,
        value: 0.0,
    };
    assert_eq!(m.boundaries, [fix(1, 6)]);
    assert_eq!(m.steps.len(), 2);
    let pull = |magnitude| Cload {
        target: Target::Node(101),
        dof: 1,
        magnitude,
    };
    let (s1, s2) = (&m.steps[0], &m.steps[1]);
    assert_eq!(s1.name.as_deref(), Some("PULL-BY-COUPLING"));
    assert_eq!(s2.name.as_deref(), Some("PULL-BY-PRESSURE"));
    assert!(s1.static_procedure.is_some() && s2.static_procedure.is_some());
    assert_eq!((s1.cloads.clone(), s1.dloads.len()), (vec![pull(-10.0)], 0));
    assert_eq!(s2.boundaries, [fix(1, 1)]);
    assert_eq!(s2.cloads, [pull(0.0)]);
    let pressure = Dload {
        target: ElementTarget::Set("EA".into()),
        face: 6,
        pressure: -10.0,
    };
    assert_eq!(s2.dloads, [pressure]);
}

/// Exact u_x: zero at the clamp x = 3, then strain 10 / E in each cube.
fn exact_ux(x: f64) -> f64 {
    if x <= 1.0 {
        -0.015 - 0.01 * (1.0 - x)
    } else if x <= 2.0 {
        -0.01 - 0.005 * (2.0 - x)
    } else {
        -0.01 * (3.0 - x)
    }
}

/// Relative tolerance on 0.025, the largest displacement, and on the stress 10. The answer
/// is exact for every element, so only rounding is left: the measured error is near 2e-14.
const TOL: f64 = 1e-10;

#[test]
fn the_model_solves_to_a_uniform_tension_in_both_steps() {
    let m = model();
    let s = solve(&m).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.steps.len(), 2);
    let end = ids(&m, "END");
    for (k, r) in s.steps.iter().enumerate() {
        let mut err = 0.0f64;
        for (n, (u, ur)) in m.nodes.iter().zip(r.displacement.iter().zip(&r.rotation)) {
            let want = match n.id {
                100 => 0.0,
                101 => -0.025,
                _ => exact_ux(n.xyz[0]),
            };
            err = err.max((u[0] - want).abs() / 0.025);
            for d in 0..3 {
                err = err
                    .max(ur[d].abs())
                    .max(if d > 0 { u[d].abs() / 0.025 } else { 0.0 });
            }
        }
        for (st, mises) in r.element_stress.iter().zip(&r.mises) {
            err = err.max((st[0] - 10.0).abs() / 10.0);
            err = err.max((mises - 10.0).abs() / 10.0);
            for v in &st[1..] {
                err = err.max(v.abs() / 10.0);
            }
        }
        // The clamp holds the bar with 10 along x, through the nodes of the end face.
        let mut sum = [0.0; 3];
        for (n, rf) in m.nodes.iter().zip(&r.reaction) {
            if end.contains(&n.id) {
                for d in 0..3 {
                    sum[d] += rf[d];
                }
            }
        }
        err = err.max((sum[0] - 10.0).abs() / 10.0);
        err = err.max(sum[1].abs().max(sum[2].abs()) / 10.0);
        eprintln!("step {}: largest error {err:e}", k + 1);
        assert!(err <= TOL, "step {}: error {err:e}", k + 1);
    }
}

/// The text with the first line that is `from` changed to `to`, and the number of that line.
fn broken(from: &str, to: &str) -> (String, usize) {
    let t = text();
    let mut lines: Vec<&str> = t.lines().collect();
    let at = lines.iter().position(|l| *l == from).unwrap();
    lines[at] = to;
    (lines.join("\n"), at + 1)
}

#[test]
fn a_missing_set_a_missing_material_and_a_bad_element_type_are_errors() {
    let cases = [
        (
            "*SOLID SECTION, ELSET=EC, MATERIAL=SOFT",
            "*SOLID SECTION, ELSET=EC_MISSING, MATERIAL=SOFT",
            "element set 'EC_MISSING' is not defined",
        ),
        (
            "START",
            "START_MISSING",
            "node set 'START_MISSING' is not defined",
        ),
        (
            "*SOLID SECTION, ELSET=EB, MATERIAL=STIFF",
            "*SOLID SECTION, ELSET=EB, MATERIAL=STEEL",
            "material 'STEEL' is not defined",
        ),
        (
            "*ELEMENT, TYPE=C3D8, ELSET=EB",
            "*ELEMENT, TYPE=C3D9, ELSET=EB",
            "unsupported element type C3D9",
        ),
    ];
    for (from, to, want) in cases {
        let (t, line) = broken(from, to);
        let e = parse(&t).unwrap_err().to_string();
        assert!(e.contains(want), "{to}: {e}");
        assert!(e.contains(&format!("line {line}")), "{to}: {e}");
    }
}
