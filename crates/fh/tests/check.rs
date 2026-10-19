use fh::check::mesh_stats;
use fh::msh::{read_msh, read_msh_shell};

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
