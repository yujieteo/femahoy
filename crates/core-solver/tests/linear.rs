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

#[test]
fn mpc_pin_transfers_load_and_equalises_all_translations() {
    let model = cube(
        "C3D8I",
        "*NODE\n9,1,1,1\n*MPC\nPIN, 9, 7\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n\
*CLOAD\n2,1,25\n3,1,25\n6,1,25\n9,1,25\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    // The pin transfers 25 N to corner 7. Uniform tension gives u = (0.1, -0.03, -0.03).
    for id in [7, 9] {
        let got = u(&model, &sol, 0, id);
        for (i, want) in [0.1, -0.03, -0.03].into_iter().enumerate() {
            assert!(
                (got[i] - want).abs() < 1e-12,
                "node {id} component {i}: {got:?}"
            );
        }
    }
    let reaction: f64 = sol.steps[0].reaction.iter().map(|r| r[0]).sum();
    assert!((reaction + 100.0).abs() < 1e-9, "reaction {reaction}");
}

#[test]
fn mpc_beam_moves_the_slave_with_the_rotation_of_the_master() {
    // Node 9 is the master. It moves by d and turns by theta. The beam is rigid, so the
    // slave node 7 moves by d + theta x r, with r from node 9 to node 7, and turns by theta.
    let model = cube(
        "C3D8I",
        "*NODE\n9,3,2,0.5\n*MPC\nBEAM, 9, 7\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n\
2,3,3\n3,3,3\n9,1,1,0.1\n9,2,2,-0.2\n9,3,3,0.3\n9,4,4,0.01\n9,5,5,0.02\n9,6,6,0.03\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    // r = (1,1,1) - (3,2,0.5) = (-2,-1,0.5). theta x r = (0.04, -0.065, 0.03).
    let want = [0.1 + 0.04, -0.2 - 0.065, 0.3 + 0.03];
    let got = u(&model, &sol, 0, 7);
    for i in 0..3 {
        assert!((got[i] - want[i]).abs() < 1e-12, "u7 {got:?} vs {want:?}");
    }
    let i7 = model.nodes.iter().position(|n| n.id == 7).unwrap();
    let rot = sol.steps[0].rotation[i7];
    for (i, want) in [0.01, 0.02, 0.03].into_iter().enumerate() {
        assert!((rot[i] - want).abs() < 1e-12, "rotation of node 7: {rot:?}");
    }
}

#[test]
fn mpc_beam_carries_an_axial_load_into_the_body_and_the_reactions_balance() {
    // Node 9 lies 2 beyond corner 7 on the x line. A load of 25 on node 9 goes through the
    // rigid beam to corner 7, with no moment because the load acts along the beam. With 25 on
    // each of nodes 2, 3 and 6, the body is in uniform tension: u = (0.1, -0.03, -0.03) at
    // corner 7. Node 9 turns by 0.01 about z, so it moves 0.02 in y relative to corner 7.
    let model = cube(
        "C3D8I",
        "*NODE\n9,3,1,1\n*MPC\nBEAM, 7, 9\n*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n\
2,3,3\n3,3,3\n9,4,5\n9,6,6,0.01\n*CLOAD\n2,1,25\n3,1,25\n6,1,25\n9,1,25\n*END STEP\n",
    );
    let sol = solve(&model).unwrap();
    let want7 = [0.1, -0.03, -0.03];
    // For theta = (0, 0, 0.01) and r = (-2, 0, 0) from node 7 to node 9, theta x r = (0, -0.02, 0).
    let want9 = [0.1, -0.03 + 0.02, -0.03];
    for (id, want) in [(7, want7), (9, want9)] {
        let got = u(&model, &sol, 0, id);
        for i in 0..3 {
            assert!(
                (got[i] - want[i]).abs() < 1e-12,
                "node {id} component {i}: {got:?} vs {want:?}"
            );
        }
    }
    for id in [7, 9] {
        let i = model.nodes.iter().position(|n| n.id == id).unwrap();
        let rot = sol.steps[0].rotation[i];
        assert!(
            rot[0].abs() < 1e-12 && rot[1].abs() < 1e-12 && (rot[2] - 0.01).abs() < 1e-12,
            "rotation of node {id}: {rot:?}"
        );
    }
    // The 4 loads of 25 are in balance with the reactions at the fixed nodes 1, 4, 5 and 8.
    let rf = &sol.steps[0].reaction;
    let index = |id: u32| model.nodes.iter().position(|n| n.id == id).unwrap();
    let fixed: f64 = [1, 4, 5, 8].into_iter().map(|id| rf[index(id)][0]).sum();
    let total: f64 = rf.iter().map(|r| r[0]).sum();
    assert!((total + 100.0).abs() < 1e-9, "sum of reactions {total}");
    assert!(
        (fixed.abs() - 100.0).abs() < 1e-9,
        "reactions at supports {fixed}"
    );
}

#[test]
fn three_instances_of_one_part_solve_to_the_exact_field_of_each_instance() {
    // I1 and I2 are in series through *MPC TIE and carry 100. I3 is alone and carries 50.
    let model = fixture("m2-instances.inp");
    assert_eq!((model.nodes.len(), model.elements.len()), (24, 3));
    assert_eq!(model.instances.len(), 3);
    let sol = solve(&model).unwrap_or_else(|e| panic!("{e}"));
    let step = &sol.steps[0];
    let name = |id: u32| model.node_instance(id).unwrap().0.name.as_str();
    // The two linked faces are the same points of space, so the tie joins instances.
    for (a, b) in [(2, 9), (3, 12), (6, 13), (7, 16)] {
        assert_eq!((name(a), name(b)), ("I1", "I2"));
    }
    let mut count = [0; 3];
    for (i, node) in model.nodes.iter().enumerate() {
        let [x, y, z] = node.xyz;
        let (k, want) = match name(node.id) {
            "I1" => (0, [0.1 * x, -0.03 * y, -0.03 * z]),
            "I2" => (1, [0.1 * x, -0.03 * y, -0.03 * z]),
            "I3" => (2, [0.05 * x, -0.015 * (y - 5.0), -0.015 * z]),
            other => panic!("node {} is in instance {other}", node.id),
        };
        count[k] += 1;
        let got = step.displacement[i];
        for c in 0..3 {
            assert!(
                (got[c] - want[c]).abs() < 1e-12,
                "node {} ({}) component {c}: {got:?} vs {want:?}",
                node.id,
                name(node.id)
            );
        }
        assert_eq!(step.rotation[i], [0.0; 3], "node {}", node.id);
    }
    assert_eq!(count, [8, 8, 8]);
    // One element per instance. The stress is uniform tension: 100, 100 and 50.
    for (id, sxx) in [(1, 100.0), (2, 100.0), (3, 50.0)] {
        let s = step.element_stress[model.elements.iter().position(|e| e.id == id).unwrap()];
        let inst = model.element_instance(id).unwrap().0;
        assert_eq!(inst.name, format!("I{id}"));
        assert!((s[0] - sxx).abs() < 1e-8 * sxx, "element {id}: {s:?}");
        for c in 1..6 {
            assert!(s[c].abs() < 1e-8 * sxx, "element {id}: {s:?}");
        }
    }
    // The loads add to 150, and the reactions balance them.
    let rx: f64 = step.reaction.iter().map(|r| r[0]).sum();
    assert!((rx + 150.0).abs() < 1e-9, "sum of reactions {rx}");
}
