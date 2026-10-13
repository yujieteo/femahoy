//! Element-face surfaces: a coupling that names an element surface uses the nodes of its faces.

use fh_core_solver::solve;
use fh_inp::{parse, Model};

const CUBE: &str =
    "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n\
9,3,0.5,0.5\n*ELEMENT, TYPE=C3D8I\n1,1,2,3,4,5,6,7,8\n*ELSET, ELSET=E\n1\n\
*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=E, MATERIAL=M\n";

fn pulled(surface: &str) -> Model {
    parse(&format!(
        "{CUBE}{surface}*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*KINEMATIC\n\
*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,3\n5,1,3\n8,1,3\n*CLOAD\n9,1,100\n*END STEP\n"
    ))
    .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn coupling_on_an_element_face_equals_coupling_on_its_nodes() {
    let by_nodes = pulled("*SURFACE, TYPE=NODE, NAME=S\n2\n3\n6\n7\n");
    let by_face = pulled("*SURFACE, TYPE=ELEMENT, NAME=S\nE, S4\n");
    let a = solve(&by_nodes).unwrap();
    let b = solve(&by_face).unwrap();
    assert_eq!(a.steps[0].displacement, b.steps[0].displacement);
    // The reference node moved along x, so the test is not empty.
    assert!(b.steps[0].displacement[8][0] > 0.0);
}

#[test]
fn a_missing_surface_is_an_error() {
    let m = parse(&format!("{CUBE}*SURFACE, TYPE=ELEMENT, NAME=T\nE, S4\n")).unwrap();
    assert!(m.element_surface("S").is_none());
}
