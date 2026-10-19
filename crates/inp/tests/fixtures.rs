//! Read the inputs of the shipped validation cases.

use fh_inp::{parse, Target};
use std::path::Path;

fn cases() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../validation/cases")
}

#[test]
fn cantilever_is_read() {
    let m = parse(
        &fh_inp::flatten(&cases().join("cantilever-c3d10/job.inp"))
            .unwrap()
            .text,
    );
    assert_eq!(m.nodes.len(), 429);
    assert_eq!(m.elements.len(), 192);
    assert_eq!(m.elset("BEAM").unwrap().ids.len(), 192);
    let step = &m.steps[0];
    assert_eq!(step.name.as_deref(), Some("TIP-LOAD"));
    assert_eq!(step.boundaries[0].target, Target::Set("FIXED".into()));
    assert_eq!(
        (step.boundaries[0].first_dof, step.boundaries[0].last_dof),
        (1, 3)
    );
    assert_eq!(step.cloads[0].dof, 3);
}

#[test]
fn every_case_input_has_a_mesh_and_units() {
    let mut n = 0;
    for case in std::fs::read_dir(cases()).unwrap() {
        let dir = case.unwrap().path();
        if !dir.join("job.inp").exists() {
            continue;
        }
        let text = fh_inp::flatten(&dir.join("job.inp")).unwrap().text;
        let m = parse(&text);
        assert!(!m.elements.is_empty(), "{}", dir.display());
        assert!(!fh_inp::units(&text).is_empty(), "{}", dir.display());
        n += 1;
    }
    assert_eq!(n, 9);
}
