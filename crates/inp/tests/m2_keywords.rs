//! The M2 keywords: sections, surfaces, couplings, equations, MPCs and pressure loads.

use fh_inp::{parse, CouplingKind, ElementTarget, EquationTerm, Model, MpcKind, SolidSection};

const MESH: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
*NODE\n9,5,5,5\n\
*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,8\n\
*ELSET, ELSET=ALL\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000,0.3\n";

fn ok(tail: &str) -> Model {
    parse(&format!("{MESH}{tail}")).unwrap_or_else(|e| panic!("{e}"))
}

fn err(tail: &str) -> String {
    parse(&format!("{MESH}{tail}")).unwrap_err().to_string()
}

#[test]
fn solid_section_links_set_and_material() {
    let m = ok("*SOLID SECTION, ELSET=all, MATERIAL=m\n");
    assert_eq!(
        m.sections,
        vec![SolidSection {
            elset: "ALL".into(),
            material: "M".into()
        }]
    );
}

#[test]
fn solid_section_checks_names() {
    assert!(err("*SOLID SECTION, ELSET=NOPE, MATERIAL=M\n").contains("element set 'NOPE'"));
    assert!(err("*SOLID SECTION, ELSET=ALL, MATERIAL=NOPE\n").contains("material 'NOPE'"));
    assert!(err("*SOLID SECTION, ELSET=ALL\n").contains("MATERIAL is required"));
}

#[test]
fn node_surface_takes_nodes_and_sets() {
    let m = ok("*NSET, NSET=TOP\n5,6,7,8\n*SURFACE, TYPE=NODE, NAME=S\nTOP\n1\n");
    assert_eq!(m.surfaces[0].name, "S");
    assert_eq!(m.surfaces[0].ids, vec![5, 6, 7, 8, 1]);
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\n").contains("unsupported TYPE ELEMENT"));
}

#[test]
fn a_surface_line_with_more_than_1_field_is_an_error() {
    for data in ["1,2\n", "1,\n2\n", "TOP,1.0\n"] {
        let e = err(&format!(
            "*NSET, NSET=TOP\n5\n*SURFACE, TYPE=NODE, NAME=S\n{data}"
        ));
        assert!(
            e.starts_with("line 22, *SURFACE") && e.contains("1 node"),
            "{e}"
        );
    }
}

#[test]
fn a_duplicate_surface_name_is_an_error() {
    // The solver takes the first surface of a name, so it would not see the second.
    let e = err("*SURFACE, TYPE=NODE, NAME=S\n1\n*SURFACE, TYPE=NODE, NAME=s\n2\n");
    assert!(
        e.starts_with("line 21, *SURFACE") && e.contains("'S'"),
        "{e}"
    );
}

#[test]
fn kinematic_coupling() {
    let m = ok("*SURFACE, TYPE=NODE, NAME=S\n1\n2\n\
*COUPLING, CONSTRAINT NAME=C1, REF NODE=9, SURFACE=S\n*KINEMATIC\n1, 3\n");
    let c = &m.couplings[0];
    assert_eq!(
        (c.name.as_str(), c.ref_node, c.surface.as_str()),
        ("C1", 9, "S")
    );
    assert_eq!(c.kind, CouplingKind::Kinematic);
    assert_eq!((c.first_dof, c.last_dof), (1, 3));
}

#[test]
fn coupling_without_data_uses_all_dofs() {
    let m = ok("*SURFACE, TYPE=NODE, NAME=S\n1\n\
*COUPLING, CONSTRAINT NAME=C1, REF NODE=9, SURFACE=S\n*DISTRIBUTING\n");
    assert_eq!(m.couplings[0].kind, CouplingKind::Distributing);
    assert_eq!((m.couplings[0].first_dof, m.couplings[0].last_dof), (1, 6));
}

#[test]
fn coupling_errors() {
    let s = "*SURFACE, TYPE=NODE, NAME=S\n1\n";
    assert!(err(&format!(
        "{s}*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n"
    ))
    .contains("needs *KINEMATIC or *DISTRIBUTING"));
    assert!(err(&format!(
        "{s}*COUPLING, CONSTRAINT NAME=C, REF NODE=99, SURFACE=S\n"
    ))
    .contains("node 99 is not defined"));
    assert!(err(&format!(
        "{s}*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=T\n"
    ))
    .contains("surface 'T' is not defined"));
    assert!(err("*KINEMATIC\n").contains("needs an open *COUPLING"));
}

#[test]
fn equation_reads_terms_over_lines() {
    let m = ok("*EQUATION\n3\n1, 1, 1.0\n2, 1, -0.5\n3, 2, -0.5\n");
    assert_eq!(
        m.equations[0].terms[1],
        EquationTerm {
            node: 2,
            dof: 1,
            coefficient: -0.5
        }
    );
    assert_eq!(m.equations[0].terms.len(), 3);
}

#[test]
fn equation_errors() {
    assert!(err("*EQUATION\n2\n1, 1, 1.0\n").contains("needs 2 terms"));
    assert!(err("*EQUATION\n1\n99, 1, 1.0\n").contains("node 99 is not defined"));
    assert!(err("*EQUATION\n0\n").contains("at least 1 term"));
}

#[test]
fn an_empty_equation_field_is_an_error_and_does_not_move_the_terms() {
    // Without the empty field, the terms move: node 2 goes away and node 1 gets fixed.
    let e = err("*EQUATION\n2\n1,1,,2,1,1,-1\n");
    assert!(e.contains("needs 2 terms"), "{e}");
    let e = err("*EQUATION\n2\n1,1,,2,1,-1\n");
    assert!(
        e.contains("line 21, *EQUATION") && e.contains("found ''"),
        "{e}"
    );
}

#[test]
fn the_count_line_of_an_equation_has_only_the_count() {
    let e = err("*EQUATION\n1,99\n1,1,1\n");
    assert!(
        e.contains("line 20, *EQUATION") && e.contains("number of terms"),
        "{e}"
    );
}

/// Model data in each place after the first `*STEP` starts.
const MODEL_DATA: [&str; 5] = [
    "*EQUATION\n1\n1,1,1\n",
    "*MPC\nTIE,1,2\n",
    "*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*KINEMATIC\n",
    "*SOLID SECTION, ELSET=ALL, MATERIAL=M\n",
    "*SURFACE, TYPE=NODE, NAME=T\n2\n",
];

#[test]
fn model_data_inside_or_after_a_step_is_an_error() {
    only_before_the_first_step(&MODEL_DATA);
}

#[test]
fn mesh_and_material_data_inside_or_after_a_step_is_an_error() {
    only_before_the_first_step(&[
        "*NODE\n10,0,0,0\n",
        "*ELEMENT, TYPE=C3D8\n2,1,2,3,4,5,6,7,8\n",
        "*NSET, NSET=N\n1\n",
        "*ELSET, ELSET=E\n1\n",
        "*MATERIAL, NAME=Q\n*ELASTIC\n1000,0.3\n",
    ]);
}

/// Each `data` passes before the first step and fails inside and after it.
fn only_before_the_first_step(model_data: &[&str]) {
    let surface = "*SURFACE, TYPE=NODE, NAME=S\n1\n";
    for data in model_data {
        ok(&format!("{surface}{data}*STEP\n*STATIC\n*END STEP\n"));
        for text in [
            format!("{surface}*STEP\n*STATIC\n{data}*END STEP\n"),
            format!("{surface}*STEP\n*STATIC\n*END STEP\n{data}"),
        ] {
            let e = err(&text);
            assert!(e.contains("before the first *STEP"), "{text}\n{e}");
        }
    }
}

#[test]
fn mpc_types() {
    let m = ok("*MPC\nTIE, 1, 2\nLINK, 3, 4\nBEAM, 5, 6\nPIN, 7, 8\n");
    let kinds: Vec<_> = m.mpcs.iter().map(|x| x.kind).collect();
    assert_eq!(
        kinds,
        [MpcKind::Tie, MpcKind::Link, MpcKind::Beam, MpcKind::Pin]
    );
    assert_eq!(m.mpcs[1].nodes, vec![3, 4]);
    assert!(err("*MPC\nSLIDER, 1, 2\n").contains("unsupported MPC type SLIDER"));
    assert!(err("*MPC\nTIE, 1\n").contains("needs 2 nodes"));
}

#[test]
fn dload_pressure_on_face() {
    let m = ok("*STEP\n*STATIC\n*DLOAD\nALL, P3, 2.5\n1, P6, -1\n*END STEP\n");
    let d = &m.steps[0].dloads;
    assert_eq!(d[0].target, ElementTarget::Set("ALL".into()));
    assert_eq!((d[0].face, d[0].pressure), (3, 2.5));
    assert_eq!(d[1].target, ElementTarget::Element(1));
    assert_eq!(d[1].face, 6);
}

#[test]
fn dload_errors() {
    let pre = "*STEP\n*STATIC\n";
    assert!(err(&format!("{pre}*DLOAD\nALL, P7, 1\n")).contains("face"));
    assert!(err(&format!("{pre}*DLOAD\nALL, GRAV, 1\n")).contains("unsupported load type GRAV"));
    assert!(err("*DLOAD\nALL, P1, 1\n").contains("needs an open *STEP"));
}
