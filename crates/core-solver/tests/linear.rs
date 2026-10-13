//! Linear static solutions with small models: patch test, tension bar, constraints.

use fh_core_solver::{solve, Solution};
use fh_inp::{parse, Model};

fn fixture(name: &str) -> Model {
    let path = format!("{}/../../fixtures/inp/{name}", env!("CARGO_MANIFEST_DIR"));
    parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn u(model: &Model, sol: &Solution, step: usize, id: u32) -> [f64; 3] {
    let i = model.nodes.iter().position(|n| n.id == id).unwrap();
    sol.steps[step].displacement[i]
}

/// Unit cube, one element of `ty`, with a bar along x. `tail` adds boundary and load data.
fn cube(ty: &str, tail: &str) -> Model {
    let text = format!(
        "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
*ELEMENT, TYPE={ty}\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n{tail}"
    );
    parse(&text).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn patch_test_reproduces_the_linear_field() {
    // Outer nodes follow u = A x. Free interior nodes must follow it too.
    let model = fixture("patch-test.inp");
    let sol = solve(&model).unwrap();
    let a = [
        [0.001, 0.0002, 0.0001],
        [0.0003, 0.0005, 0.0001],
        [0.0002, 0.0001, 0.0004],
    ];
    for node in &model.nodes {
        let got = u(&model, &sol, 0, node.id);
        for i in 0..3 {
            let want: f64 = (0..3).map(|j| a[i][j] * node.xyz[j]).sum();
            assert!(
                (got[i] - want).abs() < 1e-12,
                "node {} comp {i}: {} vs {want}",
                node.id,
                got[i]
            );
        }
    }
    // Stress is the same in every element: sigma = D * eps.
    let (e, nu) = (210000.0, 0.25);
    let lam = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let mu = e / (2.0 * (1.0 + nu));
    let tr = a[0][0] + a[1][1] + a[2][2];
    let sxx = lam * tr + 2.0 * mu * a[0][0];
    let sxy = mu * (a[0][1] + a[1][0]);
    for s in &sol.steps[0].element_stress {
        assert!((s[0] - sxx).abs() < 1e-8 * sxx, "{s:?}");
        assert!((s[3] - sxy).abs() < 1e-8 * sxy.abs(), "{s:?}");
    }
}

#[test]
fn tension_bar_matches_the_exact_solution() {
    // Face x=0 slides, one corner pinned. Total force 100 on face x=1, area 1.
    for ty in ["C3D8", "C3D8I"] {
        let model = cube(
            ty,
            "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n\
*CLOAD\n2,1,25\n3,1,25\n6,1,25\n7,1,25\n*END STEP\n",
        );
        let sol = solve(&model).unwrap();
        // E = 1000, sigma = 100: strain 0.1. Lateral strain -0.03.
        let tip = u(&model, &sol, 0, 7);
        assert!((tip[0] - 0.1).abs() < 1e-12, "{ty} {tip:?}");
        assert!((tip[1] + 0.03).abs() < 1e-12, "{ty} {tip:?}");
        assert!((tip[2] + 0.03).abs() < 1e-12, "{ty} {tip:?}");
        let rf = &sol.steps[0].reaction;
        let total: f64 = (0..8).map(|i| rf[i][0]).sum();
        assert!((total + 100.0).abs() < 1e-9, "{ty} reaction {total}");
    }
}

#[test]
fn pressure_on_a_face_loads_the_body() {
    // Pressure 10 on face 4 (x=1 side) pushes toward -x. Bar is fixed at x=0 in x.
    let model = cube(
        "C3D8I",
        "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n\
*DLOAD\nE, P4, -10\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    // Negative pressure pulls: sigma_xx = +10, strain 0.01.
    let tip = u(&model, &sol, 0, 7);
    assert!((tip[0] - 0.01).abs() < 1e-12, "{tip:?}");
}

#[test]
fn equation_ties_two_dofs() {
    // u_x(node 2) = 2 * u_x(node 6) keeps the solve consistent with the load split.
    let model = cube(
        "C3D8I",
        "*EQUATION\n2\n2,1,1.0\n6,1,-2.0\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n\
*CLOAD\n3,1,50\n7,1,50\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    let (a, b) = (u(&model, &sol, 0, 2), u(&model, &sol, 0, 6));
    assert!((a[0] - 2.0 * b[0]).abs() < 1e-12, "{a:?} {b:?}");
}

#[test]
fn kinematic_coupling_moves_slaves_rigidly() {
    // Ref node 9 rotates about z by 0.01 and moves 0.2 in x. Face x=1 follows.
    let text = "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
9,1,0.5,0.5\n*ELEMENT, TYPE=C3D8I\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n\
*NSET, NSET=FIX\n1,4,5,8\n*SURFACE, TYPE=NODE, NAME=S\n2\n3\n6\n7\n\
*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*KINEMATIC\n\
*STEP\n*STATIC\n*BOUNDARY\nFIX,1,3\n9,1,1,0.2\n9,2,3\n9,4,5\n9,6,6,0.01\n*END STEP\n";
    let model = parse(text).unwrap();
    let sol = solve(&model).unwrap();
    // Node 3 at (1,1,0): r = (0, 0.5, -0.5). theta x r = (-0.01*0.5, 0, 0).
    let got = u(&model, &sol, 0, 3);
    assert!((got[0] - (0.2 - 0.005)).abs() < 1e-12, "{got:?}");
    assert!(got[1].abs() < 1e-12 && got[2].abs() < 1e-12, "{got:?}");
    let got = u(&model, &sol, 0, 2);
    assert!((got[0] - (0.2 + 0.005)).abs() < 1e-12, "{got:?}");
}

#[test]
fn distributing_coupling_averages_the_slaves() {
    // Slaves are pulled by prescribed x motion; ref node follows the average.
    let text = "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
9,3,0.5,0.5\n*ELEMENT, TYPE=C3D8I\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n\
*NSET, NSET=FIX\n1,4,5,8\n*SURFACE, TYPE=NODE, NAME=S\n2\n3\n6\n7\n\
*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*DISTRIBUTING\n1,3\n\
*STEP\n*STATIC\n*BOUNDARY\nFIX,1,3\n2,1,1,0.1\n3,1,1,0.2\n6,1,1,0.3\n7,1,1,0.4\n\
2,2,3\n3,2,3\n6,2,3\n7,2,3\n*END STEP\n";
    let model = parse(text).unwrap();
    let sol = solve(&model).unwrap();
    let got = u(&model, &sol, 0, 9);
    assert!((got[0] - 0.25).abs() < 1e-12, "{got:?}");
}

#[test]
fn mpc_link_keeps_the_distance() {
    // Node 7 may only move normal to the line from node 1 (the diagonal).
    let model = cube(
        "C3D8I",
        "*MPC\nLINK, 1, 7\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n2,2,3\n3,3,3\n4,1,1\n5,1,2\n6,2,2\n8,1,1\n\
*CLOAD\n7,1,10\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    let d = u(&model, &sol, 0, 7);
    let along = (d[0] + d[1] + d[2]) / 3.0_f64.sqrt();
    assert!(along.abs() < 1e-12, "{d:?}");
}

#[test]
fn mpc_tie_equalises_translations() {
    let model = cube(
        "C3D8I",
        "*MPC\nTIE, 2, 3\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n\
*CLOAD\n6,1,30\n7,1,30\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    let (a, b) = (u(&model, &sol, 0, 2), u(&model, &sol, 0, 3));
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 1e-12, "{a:?} {b:?}");
    }
}

#[test]
fn missing_boundary_conditions_are_reported() {
    let model = cube("C3D8", "*STEP\n*STATIC\n*CLOAD\n7,1,1\n*END STEP\n");
    let err = solve(&model).unwrap_err().to_string();
    assert!(err.contains("not positive definite"), "{err}");
}

#[test]
fn element_without_section_is_reported() {
    let mut model = cube("C3D8", "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n*END STEP\n");
    model.sections.clear();
    let err = solve(&model).unwrap_err().to_string();
    assert!(err.contains("element 1 has no *SOLID SECTION"), "{err}");
}

#[test]
fn element_in_two_sections_is_reported() {
    // The second section gives element 1 another material. The solver must not take one.
    let model = cube(
        "C3D8",
        "*ELSET, ELSET=F\n1\n*MATERIAL, NAME=SOFT\n*ELASTIC\n1, 0.3\n\
*SOLID SECTION, ELSET=F, MATERIAL=SOFT\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n\
2,3,3\n3,3,3\n*CLOAD\n7,1,1\n*END STEP\n",
    );
    let err = solve(&model).map(|_| ()).unwrap_err().to_string();
    assert!(
        err.contains("element 1 is in 2 *SOLID SECTION element sets (E and F)"),
        "{err}"
    );
}

#[test]
fn conflicting_constraints_are_reported() {
    let model = cube(
        "C3D8",
        "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n2,1,1,0.1\n2,1,1,0.2\n*END STEP\n",
    );
    // The later value replaces the earlier one in the same step: no conflict.
    assert!(solve(&model).is_ok());
    let model = cube(
        "C3D8",
        "*EQUATION\n2\n1,1,1.0\n2,1,-1.0\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n2,1,1,0.1\n*END STEP\n",
    );
    let err = solve(&model).unwrap_err().to_string();
    assert!(err.contains("conflicting constraints"), "{err}");
}

#[test]
fn steps_accumulate_boundary_conditions() {
    let model = cube(
        "C3D8I",
        "*STEP, NAME=A\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n*CLOAD\n2,1,25\n3,1,25\n6,1,25\n7,1,25\n*END STEP\n\
*STEP, NAME=B\n*STATIC\n*CLOAD\n2,1,50\n3,1,50\n6,1,50\n7,1,50\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    assert_eq!(sol.steps.len(), 2);
    assert!((u(&model, &sol, 0, 7)[0] - 0.1).abs() < 1e-12);
    assert!((u(&model, &sol, 1, 7)[0] - 0.2).abs() < 1e-12);
}
