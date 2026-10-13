//! `*PART`, `*ASSEMBLY` and `*INSTANCE`: flat global ids and the map to instances.

use fh_inp::{parse, Model, Target};

const PART: &str = "*PART, NAME=BLOCK\n\
*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
*ELEMENT, TYPE=C3D8, ELSET=ALL\n1,1,2,3,4,5,6,7,8\n\
*NSET, NSET=TOP\n5,6,7,8\n\
*SOLID SECTION, ELSET=ALL, MATERIAL=M\n\
*END PART\n";

const MATERIAL: &str = "*MATERIAL, NAME=M\n*ELASTIC\n1000,0.3\n";

fn two_instances(extra: &str) -> String {
    format!(
        "{PART}*ASSEMBLY, NAME=A\n\
*INSTANCE, NAME=I1, PART=BLOCK\n*END INSTANCE\n\
*INSTANCE, NAME=I2, PART=BLOCK\n10,0,0\n*END INSTANCE\n\
{extra}*END ASSEMBLY\n{MATERIAL}"
    )
}

fn ok(text: &str) -> Model {
    parse(text).unwrap_or_else(|e| panic!("{e}"))
}

fn err(text: &str) -> String {
    parse(text).unwrap_err().to_string()
}

#[test]
fn instances_get_distinct_global_ids() {
    let m = ok(&two_instances(""));
    assert_eq!(m.nodes.len(), 16);
    assert_eq!(m.elements.len(), 2);
    assert_eq!(m.elements[0].nodes, (1..=8).collect::<Vec<_>>());
    assert_eq!(m.elements[1].nodes, (9..=16).collect::<Vec<_>>());
    // The translation moves the second instance.
    assert_eq!(m.nodes[8].xyz, [10.0, 0.0, 0.0]);
    assert_eq!(m.nodes[9].xyz, [11.0, 0.0, 0.0]);
}

#[test]
fn maps_name_the_instance_and_part_label() {
    let m = ok(&two_instances(""));
    let (inst, label) = m.node_instance(11).unwrap();
    assert_eq!(
        (inst.name.as_str(), inst.part.as_str(), label),
        ("I2", "BLOCK", 3)
    );
    let (inst, label) = m.element_instance(1).unwrap();
    assert_eq!((inst.name.as_str(), label), ("I1", 1));
    assert!(m.node_instance(99).is_none());
}

#[test]
fn flat_ids_have_no_origin() {
    let m = ok("*NODE\n1,0,0,0\n");
    assert!(m.instances.is_empty());
    assert!(m.node_instance(1).is_none());
}

#[test]
fn part_sets_and_sections_are_instance_qualified() {
    let m = ok(&two_instances(""));
    assert_eq!(m.nset("I1.TOP").unwrap().ids, [5, 6, 7, 8]);
    assert_eq!(m.nset("I2.TOP").unwrap().ids, [13, 14, 15, 16]);
    assert_eq!(m.elset("I2.ALL").unwrap().ids, [2]);
    let sections: Vec<_> = m.sections.iter().map(|s| s.elset.as_str()).collect();
    assert_eq!(sections, ["I1.ALL", "I2.ALL"]);
}

#[test]
fn assembly_sets_use_part_labels() {
    let m = ok(&two_instances(
        "*NSET, NSET=BOT, INSTANCE=I2, GENERATE\n1,4\n*ELSET, ELSET=E, INSTANCE=I2\n1\n",
    ));
    assert_eq!(m.nset("BOT").unwrap().ids, [9, 10, 11, 12]);
    assert_eq!(m.elset("E").unwrap().ids, [2]);
}

#[test]
fn boundary_accepts_instance_node_and_set_names() {
    let m = ok(&format!(
        "{}*BOUNDARY\nI2.1,1,3\nI2.TOP,3\n",
        two_instances("")
    ));
    assert_eq!(m.boundaries[0].target, Target::Node(9));
    assert_eq!(m.boundaries[1].target, Target::Set("I2.TOP".into()));
}

#[test]
fn flat_and_part_data_mix() {
    let m = ok(&format!("*NODE\n1,0,0,0\n{}", two_instances("")));
    assert_eq!(m.nodes.len(), 17);
    assert!(m.node_instance(1).is_none());
    assert_eq!(m.node_instance(2).unwrap().0.name, "I1");
}

#[test]
fn part_errors_name_line_and_keyword() {
    assert_eq!(
        err("*PART, NAME=A\n"),
        "line 1, *PART: part has no *END PART (INP011)"
    );
    assert_eq!(
        err("*PART, NAME=A\n*STEP\n"),
        "line 2, *STEP: keyword is not allowed in *PART (INP010)"
    );
    assert_eq!(
        err("*END PART\n"),
        "line 1, *END PART: no open *PART (INP010)"
    );
    assert_eq!(
        err("*PART, NAME=A\n*END PART\n*PART, NAME=A\n"),
        "line 3, *PART: part A is defined twice (INP008)"
    );
    assert_eq!(
        err("*PART, NAME=A\n*NODE\n1,0,0,0\n*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,8\n"),
        "line 5, *ELEMENT: node 2 is not defined (INP009)"
    );
}

#[test]
fn assembly_errors_name_line_and_keyword() {
    assert_eq!(
        err("*ASSEMBLY, NAME=A\n"),
        "line 1, *ASSEMBLY: assembly has no *END ASSEMBLY (INP011)"
    );
    assert_eq!(
        err("*ASSEMBLY, NAME=A\n*NODE\n"),
        "line 2, *NODE: keyword is not allowed in *ASSEMBLY (INP010)"
    );
    assert_eq!(
        err("*ASSEMBLY, NAME=A\n*INSTANCE, NAME=I, PART=NOPE\n"),
        "line 2, *INSTANCE: part 'NOPE' is not defined (INP009)"
    );
    assert_eq!(
        err("*INSTANCE, NAME=I, PART=B\n"),
        "line 1, *INSTANCE: keyword needs an open *ASSEMBLY (INP010)"
    );
    let dup = format!(
        "{PART}*ASSEMBLY, NAME=A\n*INSTANCE, NAME=I, PART=BLOCK\n*END INSTANCE\n\
*INSTANCE, NAME=I, PART=BLOCK\n"
    );
    assert_eq!(
        err(&dup),
        "line 20, *INSTANCE: instance I is defined twice (INP008)"
    );
    let label = two_instances("*NSET, NSET=X, INSTANCE=I1\n9\n");
    assert!(err(&label).ends_with("node 9 is not defined in instance I1 (INP009)"));
    let rotation =
        format!("{PART}*ASSEMBLY, NAME=A\n*INSTANCE, NAME=I, PART=BLOCK\n0,0,0\n0,0,0,1,0,0,90\n");
    assert!(err(&rotation).contains("only a translation"));
}

#[test]
fn part_section_material_is_checked_at_the_end() {
    let text = format!(
        "{PART}*ASSEMBLY, NAME=A\n*INSTANCE, NAME=I, PART=BLOCK\n*END INSTANCE\n*END ASSEMBLY\n"
    );
    assert_eq!(
        err(&text),
        "line 18, *INSTANCE: material 'M' is not defined (INP009)"
    );
}
