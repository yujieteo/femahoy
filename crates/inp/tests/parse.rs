use fh_inp::{parse, ElementKind, Model, ParseError, Target};

fn load(name: &str) -> Model {
    let path = format!("{}/../../fixtures/inp/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap();
    parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn err(text: &str) -> ParseError {
    parse(text).unwrap_err()
}

#[test]
fn patch_test_fixture_parses() {
    let m = load("patch_test.inp");
    assert_eq!(m.nodes.len(), 12);
    assert_eq!(m.nodes[4].id, 5);
    assert_eq!(m.nodes[4].coords, [1.1, 0.9, 0.0]);
    assert_eq!(m.elements.len(), 2);
    assert_eq!(m.elements[0].kind, ElementKind::C3D8I);
    assert_eq!(m.elements[0].nodes, vec![1, 2, 5, 4, 7, 8, 11, 10]);
    assert_eq!(m.nsets["X0"], vec![1, 4, 7, 10]);
    assert_eq!(m.nsets["X2"], vec![3, 6, 9, 12]);
    assert_eq!(m.nsets["ALL"].len(), 12);
    assert_eq!(m.elsets["EPATCH"], vec![1, 2]);
    assert_eq!(m.materials.len(), 1);
    assert_eq!(m.materials[0].name, "TEST_MAT");
    let e = m.materials[0].elastic.unwrap();
    assert_eq!((e.young, e.poisson), (1000.0, 0.3));
    assert_eq!(m.steps.len(), 1);
    let step = &m.steps[0];
    assert_eq!(step.name.as_deref(), Some("PATCH"));
    assert!(!step.nlgeom);
    assert_eq!(step.statics.unwrap().period, Some(1.0));
    assert_eq!(step.boundaries.len(), 3);
    assert_eq!(step.boundaries[0].target, Target::Set("X0".into()));
    assert_eq!(
        (step.boundaries[0].first_dof, step.boundaries[0].last_dof),
        (1, 3)
    );
    assert_eq!(step.boundaries[1].value, 0.002);
    assert_eq!(step.boundaries[2].first_dof, 2);
    assert_eq!(step.boundaries[2].last_dof, 3);
}

#[test]
fn cantilever_fixture_parses() {
    let m = load("cantilever.inp");
    assert_eq!(m.nodes.len(), 20);
    assert_eq!(m.nodes[4].coords, [10.0, 0.0, 0.0]);
    assert_eq!(m.elements.len(), 4);
    assert_eq!(m.elements[3].nodes, vec![4, 5, 10, 9, 14, 15, 20, 19]);
    assert_eq!(m.nsets["ALL"], (1..=20).collect::<Vec<u32>>());
    assert_eq!(m.nsets["FIXED"], vec![1, 6, 11, 16]);
    assert_eq!(m.nsets["TIP"], vec![5, 10, 15, 20]);
    assert_eq!(m.elsets["EALL"], vec![1, 2, 3, 4]);
    let step = &m.steps[0];
    assert_eq!(step.boundaries.len(), 1);
    assert_eq!(step.cloads.len(), 1);
    assert_eq!(step.cloads[0].target, Target::Set("TIP".into()));
    assert_eq!(step.cloads[0].dof, 3);
    assert_eq!(step.cloads[0].magnitude, -0.25);
}

#[test]
fn element_with_wrong_node_count_names_line_and_keyword() {
    let e = err("*NODE\n1, 0, 0, 0\n*ELEMENT, TYPE=C3D8I\n1, 1, 2, 3, 4, 5, 6, 7\n");
    assert_eq!(e.line, 4);
    assert_eq!(e.keyword, "*ELEMENT");
}

#[test]
fn unsupported_keyword_names_line_and_keyword() {
    let e = err("*HEADING\nDemo\n*PART, NAME=P1\n");
    assert_eq!(e.line, 1);
    assert_eq!(e.keyword, "*HEADING");
    let e = err("*NODE\n1, 0, 0, 0\n*PART, NAME=P1\n");
    assert_eq!(e.line, 3);
    assert_eq!(e.keyword, "*PART");
}

#[test]
fn unknown_element_type_is_rejected() {
    let e = err("*ELEMENT, TYPE=C3D20\n1, 1, 2, 3\n");
    assert_eq!(e.line, 1);
    assert_eq!(e.keyword, "*ELEMENT");
}

#[test]
fn elastic_without_material_is_rejected() {
    let e = err("*ELASTIC\n1000.0, 0.3\n");
    assert_eq!(e.line, 1);
    assert_eq!(e.keyword, "*ELASTIC");
}

#[test]
fn elastic_with_unsupported_type_is_rejected() {
    let e = err("*MATERIAL, NAME=M\n*ELASTIC, TYPE=ORTHOTROPIC\n1.0, 0.3\n");
    assert_eq!(e.line, 2);
    assert_eq!(e.keyword, "*ELASTIC");
}

#[test]
fn boundary_outside_step_is_rejected() {
    let e = err("*NODE\n1, 0, 0, 0\n*BOUNDARY\n1, 1, 3\n");
    assert_eq!(e.line, 3);
    assert_eq!(e.keyword, "*BOUNDARY");
}

#[test]
fn duplicate_node_id_is_rejected() {
    let e = err("*NODE\n1, 0, 0, 0\n1, 1, 0, 0\n");
    assert_eq!(e.line, 3);
    assert_eq!(e.keyword, "*NODE");
}

#[test]
fn unknown_parameter_is_rejected() {
    let e = err("*STEP, NAME=S, FOO=1\n*STATIC\n1.0, 1.0\n*END STEP\n");
    assert_eq!(e.line, 1);
    assert_eq!(e.keyword, "*STEP");
}

#[test]
fn data_line_outside_keyword_is_rejected() {
    let e = err("1, 2, 3, 4\n");
    assert_eq!(e.line, 1);
}

#[test]
fn bad_number_names_line_and_keyword() {
    let e = err("*NODE\n1, 0.0, abc, 0.0\n");
    assert_eq!(e.line, 2);
    assert_eq!(e.keyword, "*NODE");
}

#[test]
fn keywords_are_case_insensitive_and_comments_are_skipped() {
    let m = parse("** a comment\n\n*node\n7, 1.0, 2.0, 3.0\n").unwrap();
    assert_eq!(m.nodes[0].id, 7);
    assert_eq!(m.nodes[0].coords, [1.0, 2.0, 3.0]);
}

#[test]
fn nset_generate_needs_three_values() {
    let e = err("*NSET, NSET=A, GENERATE\n1, 10\n");
    assert_eq!(e.line, 2);
    assert_eq!(e.keyword, "*NSET");
}

#[test]
fn cload_outside_step_is_rejected() {
    let e = err("*CLOAD\n1, 3, -1.0\n");
    assert_eq!(e.line, 1);
    assert_eq!(e.keyword, "*CLOAD");
}

#[test]
fn error_message_names_line_and_keyword_in_text() {
    let e = err("*HEADING\n");
    let text = e.to_string();
    assert!(text.contains("line 1"), "{text}");
    assert!(text.contains("*HEADING"), "{text}");
}
