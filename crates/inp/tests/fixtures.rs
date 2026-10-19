//! Parse the meshes of the shipped validation cases and check the typed model.

use fh_inp::{parse, Target};
use std::path::Path;

fn load(path: &Path) -> fh_inp::Model {
    let text = std::fs::read_to_string(path).unwrap();
    parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn cases() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases")
}

#[test]
fn every_case_mesh_parses_and_has_only_c3d10() {
    let mut n = 0;
    for case in std::fs::read_dir(cases()).unwrap() {
        let dir = case.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(dir).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_some_and(|e| e == "inp") {
                let m = load(&path);
                assert!(!m.elements.is_empty(), "{}", path.display());
                assert!(
                    m.elements
                        .iter()
                        .all(|e| e.element_type == "C3D10" && e.nodes.len() == 10),
                    "{}",
                    path.display()
                );
                n += 1;
            }
        }
    }
    assert_eq!(n, 9);
}

#[test]
fn cantilever_parses() {
    let m = load(&cases().join("cantilever-c3d10/mesh-8.inp"));
    assert_eq!(m.nodes.len(), 429);
    assert_eq!(m.elements.len(), 192);
    assert_eq!(m.elset("BEAM").unwrap().ids.len(), 192);
    let steel = m.material("STEEL").unwrap();
    assert!(steel.elastic.is_some());
    let step = &m.steps[0];
    assert_eq!(step.name.as_deref(), Some("TIP-LOAD"));
    assert_eq!(step.boundaries[0].target, Target::Set("FIXED".into()));
    assert_eq!(
        (step.boundaries[0].first_dof, step.boundaries[0].last_dof),
        (1, 3)
    );
    assert_eq!(step.cloads[0].dof, 3);
}
