//! Parse the shipped fixtures and check the typed model.

use fh_inp::{parse, Target};

fn load(name: &str) -> fh_inp::Model {
    let path = format!("{}/../../fixtures/inp/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn patch_test_parses() {
    let m = load("patch-test.inp");
    assert_eq!(m.nodes.len(), 16);
    assert_eq!(m.elements.len(), 7);
    assert!(m
        .elements
        .iter()
        .all(|e| e.nodes.len() == 8 && e.element_type == "C3D8"));
    assert_eq!(m.nset("OUTER").unwrap().ids, (1..=8).collect::<Vec<_>>());
    assert_eq!(m.elset("ALL").unwrap().ids.len(), 7);
    let steel = m.material("STEEL").unwrap();
    assert_eq!(steel.elastic.as_ref().unwrap().youngs_modulus, 210000.0);
    assert_eq!(steel.elastic.as_ref().unwrap().poissons_ratio, 0.25);
    assert_eq!(m.steps.len(), 1);
    assert_eq!(m.steps[0].boundaries.len(), 24);
    assert_eq!(m.nodes[1].xyz, [1.0, 0.0, 0.0]);
}

#[test]
fn cantilever_parses() {
    let m = load("cantilever.inp");
    assert_eq!(m.nodes.len(), 20);
    assert_eq!(m.elements.len(), 4);
    assert_eq!(m.elements[0].element_type, "C3D8I");
    assert_eq!(m.elset("BEAM").unwrap().ids, vec![1, 2, 3, 4]);
    assert_eq!(m.nset("TIP").unwrap().ids, vec![17, 18, 19, 20]);
    let step = &m.steps[0];
    assert_eq!(step.name.as_deref(), Some("TIP-LOAD"));
    let st = step.static_procedure.as_ref().unwrap();
    assert_eq!(st.initial_increment, Some(1.0));
    assert_eq!(st.min_increment, Some(1e-4));
    assert_eq!(step.boundaries[0].target, Target::Set("FIXED".into()));
    assert_eq!(
        (step.boundaries[0].first_dof, step.boundaries[0].last_dof),
        (1, 3)
    );
    assert_eq!(step.cloads[0].dof, 3);
    assert_eq!(step.cloads[0].magnitude, -0.25);
}
