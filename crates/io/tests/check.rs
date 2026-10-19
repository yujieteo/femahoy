use fh_inp::{Element, Model, Node};
use fh_io::{mesh_stats, read_msh, read_msh_shell};

const TET10: &str = include_str!("data/tet10.msh");
const SHELL6: &str = include_str!("data/shell6.msh");

#[test]
fn tet10_has_a_positive_volume_and_no_inverted_element() {
    let s = mesh_stats(&read_msh(TET10).unwrap()).unwrap();
    assert_eq!(s.types, [("C3D10".to_owned(), 1)]);
    assert_eq!((s.bbox_min, s.bbox_max), ([0.0; 3], [1.0; 3]));
    assert!((s.measure - 1.0 / 6.0).abs() < 1e-12, "{}", s.measure);
    assert!(s.min_scaled_jacobian > 0.0);
    assert_eq!(s.inverted, 0);
}

#[test]
fn a_mirrored_tet10_is_inverted() {
    let mut m = read_msh(TET10).unwrap();
    for n in &mut m.nodes {
        n.xyz[0] = -n.xyz[0];
    }
    assert_eq!(mesh_stats(&m).unwrap().inverted, 1);
}

#[test]
fn a_missing_node_is_an_error() {
    let mut m = read_msh(TET10).unwrap();
    m.nodes.pop();
    assert!(mesh_stats(&m).unwrap_err().contains("missing node"));
}

#[test]
fn s6_has_an_area_and_no_inverted_element() {
    let m = read_msh_shell(SHELL6).unwrap().model;
    let s = mesh_stats(&m).unwrap();
    assert_eq!(s.types, [("S6".to_owned(), 2)]);
    assert!((s.measure - 1.0).abs() < 1e-12, "{}", s.measure);
    // The smallest corner is the 45 degree corner of a half square.
    assert!((s.min_scaled_jacobian - 0.5f64.sqrt()).abs() < 1e-12);
    assert_eq!(s.inverted, 0);
}

#[test]
fn a_flat_s6_is_inverted() {
    let mut m = read_msh_shell(SHELL6).unwrap().model;
    // Put corner 3 on the line through corners 1 and 2.
    m.nodes[2].xyz = [2.0, 0.0, 0.0];
    assert_eq!(mesh_stats(&m).unwrap().inverted, 1);
}

/// One S8R from its 4 corners. The midside nodes are halfway.
fn s8r(corners: [[f64; 3]; 4]) -> Model {
    let mut xyz = corners.to_vec();
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        xyz.push([0, 1, 2].map(|k| (a[k] + b[k]) / 2.0));
    }
    Model {
        nodes: (1..=8).zip(xyz).map(|(id, xyz)| Node { id, xyz }).collect(),
        elements: vec![Element {
            id: 1,
            element_type: "S8R".into(),
            nodes: (1..=8).collect(),
        }],
        ..Model::default()
    }
}

#[test]
fn s8r_has_an_area_and_a_concave_corner_is_inverted() {
    let square = s8r([[0.0; 3], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [0.0, 2.0, 0.0]]);
    let s = mesh_stats(&square).unwrap();
    assert!((s.measure - 4.0).abs() < 1e-12 && (s.min_scaled_jacobian - 1.0).abs() < 1e-12);
    assert_eq!(s.inverted, 0);
    // The corner at (0.5, 0.5) points into the element.
    let dart = s8r([[0.0; 3], [2.0, 0.0, 0.0], [0.5, 0.5, 0.0], [0.0, 2.0, 0.0]]);
    assert_eq!(mesh_stats(&dart).unwrap().inverted, 1);
}
