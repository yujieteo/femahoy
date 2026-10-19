//! The M2 keywords: sections, element surfaces and pressure loads.

use fh_inp::{parse, ElementTarget, Model, SolidSection};

/// 1 C3D10, a set and a material. The tail starts at line 19.
const MESH: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,.5,0,0\n6,.5,.5,0\n7,0,.5,0\n8,0,0,.5\n\
9,.5,0,.5\n10,0,.5,.5\n\
*ELEMENT, TYPE=C3D10\n1,1,2,3,4,5,6,7,8,9,10\n\
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
fn element_surface_takes_elements_and_sets() {
    let m = ok("*SURFACE, TYPE=ELEMENT, NAME=S\n1, S4\nall, s3\n");
    assert_eq!(m.element_surfaces.len(), 1);
    assert_eq!(m.element_surfaces[0].name, "S");
    assert_eq!(m.element_surfaces[0].faces, vec![(1, 4), (1, 3)]);
    assert!(m.element_surface("S").is_some());
}

#[test]
fn element_surface_errors() {
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\n1, S5\n").contains("face must be 1 to 4"));
    assert!(err("*SURFACE, TYPE=FACE, NAME=S\n").contains("unsupported TYPE FACE"));
    assert!(err("*SURFACE, TYPE=NODE, NAME=S\n1\n").contains("unsupported TYPE NODE"));
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\n1, P1\n").contains("unsupported face P1"));
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\n1\n").contains("element and face"));
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\n9, S1\n").contains("element 9 is not defined"));
    assert!(err("*SURFACE, TYPE=ELEMENT, NAME=S\nNOPE, S1\n").contains("element set 'NOPE'"));
}

#[test]
fn a_duplicate_surface_name_is_an_error() {
    // The solver takes the first surface of a name, so it would not see the second.
    let e = err("*SURFACE, TYPE=ELEMENT, NAME=S\n1, S1\n*SURFACE, TYPE=ELEMENT, NAME=s\n1, S2\n");
    assert!(
        e.starts_with("line 21, *SURFACE") && e.contains("'S'"),
        "{e}"
    );
}

/// Model data in each place after the first `*STEP` starts.
const MODEL_DATA: [&str; 2] = [
    "*SOLID SECTION, ELSET=ALL, MATERIAL=M\n",
    "*SURFACE, TYPE=ELEMENT, NAME=T\n1, S2\n",
];

#[test]
fn model_data_inside_or_after_a_step_is_an_error() {
    only_before_the_first_step(&MODEL_DATA);
}

#[test]
fn mesh_and_material_data_inside_or_after_a_step_is_an_error() {
    only_before_the_first_step(&[
        "*NODE\n11,0,0,0\n",
        "*ELEMENT, TYPE=C3D10\n2,1,2,3,4,5,6,7,8,9,10\n",
        "*NSET, NSET=N\n1\n",
        "*ELSET, ELSET=E\n1\n",
        "*MATERIAL, NAME=Q\n*ELASTIC\n1000,0.3\n",
    ]);
}

/// Each `data` passes before the first step and fails inside and after it.
fn only_before_the_first_step(model_data: &[&str]) {
    for data in model_data {
        ok(&format!("{data}*STEP\n*STATIC\n*END STEP\n"));
        for text in [
            format!("*STEP\n*STATIC\n{data}*END STEP\n"),
            format!("*STEP\n*STATIC\n*END STEP\n{data}"),
        ] {
            let e = err(&text);
            assert!(e.contains("before the first *STEP"), "{text}\n{e}");
        }
    }
}

#[test]
fn pressure_types_ignore_case() {
    let m = ok("*STEP\n*STATIC\n*DLOAD\nall, p3, 2.5\n*END STEP\n");
    assert_eq!(m.steps[0].dloads[0].face, 3);
}

#[test]
fn dload_pressure_on_face() {
    let m = ok("*STEP\n*STATIC\n*DLOAD\nALL, P3, 2.5\n1, P4, -1\n*END STEP\n");
    let d = &m.steps[0].dloads;
    assert_eq!(d[0].target, ElementTarget::Set("ALL".into()));
    assert_eq!((d[0].face, d[0].pressure), (3, 2.5));
    assert_eq!(d[1].target, ElementTarget::Element(1));
    assert_eq!(d[1].face, 4);
}

#[test]
fn dload_errors() {
    let pre = "*STEP\n*STATIC\n";
    assert!(err(&format!("{pre}*DLOAD\nALL, P5, 1\n")).contains("face must be 1 to 4"));
    assert!(err(&format!("{pre}*DLOAD\nALL, GRAV, 1\n")).contains("unsupported load type GRAV"));
    assert!(err("*DLOAD\nALL, P1, 1\n").contains("needs an open *STEP"));
}

#[test]
fn dsload_reads_a_pressure_on_an_element_surface() {
    let m = ok(
        "*SURFACE, TYPE=ELEMENT, NAME=S\n1, S4\n*STEP\n*STATIC\n*DSLOAD\nS, P, 2.5\n*END STEP\n",
    );
    let d = &m.steps[0].dsloads;
    assert_eq!((d[0].surface.as_str(), d[0].pressure), ("S", 2.5));
}

#[test]
fn dsload_finds_the_surface_in_any_letter_case() {
    // NAME= is upper case after the parse, a data line keeps its letters. Names ignore case.
    let m = ok(
        "*SURFACE, TYPE=ELEMENT, NAME=Load\n1, S4\n*STEP\n*STATIC\n*DSLOAD\nload, P, 2.5\n*END STEP\n",
    );
    let d = &m.steps[0].dsloads;
    assert_eq!((d[0].surface.as_str(), d[0].pressure), ("LOAD", 2.5));
    assert!(m.element_surface("load").is_some());
}

#[test]
fn dsload_errors() {
    let step = "*STEP\n*STATIC\n*DSLOAD\n";
    let s = "*SURFACE, TYPE=ELEMENT, NAME=S\n1, S4\n";
    assert!(err(&format!("{s}{step}NOPE, P, 1\n")).contains("'NOPE' is not defined"));
    assert!(err(&format!("{s}{step}S, P1, 1\n")).contains("unsupported load type P1"));
    assert!(err(&format!("{s}*DSLOAD\nS, P, 1\n")).contains("needs an open *STEP"));
}

#[test]
fn parse_all_collects_errors_with_codes_and_file() {
    let text = format!(
        "{MESH}*PLASTIC\n*ELASTIC, FOO=1\n1000,0.3\n*SOLID SECTION, ELSET=NONE, MATERIAL=M\n"
    );
    let errors = fh_inp::parse_file("bad.inp", &text).unwrap_err();
    let codes: Vec<_> = errors.iter().map(|e| e.code).collect();
    assert_eq!(codes, ["INP002", "INP003", "INP009"]);
    assert!(errors.iter().all(|e| e.file.as_deref() == Some("bad.inp")));
    assert!(errors[0].to_string().starts_with("bad.inp: line "));
    assert_eq!(parse(&text).unwrap_err().code, "INP002");
}
