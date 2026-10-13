use fh_io::{mesh_stats, read_msh};

const HEX: &str = include_str!("data/hex.msh");
const TET10: &str = include_str!("data/tet10.msh");

#[test]
fn unit_cube_hex_has_volume_1_and_scaled_jacobian_1() {
    let s = mesh_stats(&read_msh(HEX).unwrap()).unwrap();
    assert_eq!(s.types, [("C3D8I".to_owned(), 1)]);
    assert_eq!((s.bbox_min, s.bbox_max), ([0.0; 3], [1.0; 3]));
    assert!((s.volume - 1.0).abs() < 1e-12, "{}", s.volume);
    assert!((s.min_scaled_jacobian - 1.0).abs() < 1e-12);
    assert_eq!(s.inverted, 0);
}

#[test]
fn tet10_has_a_positive_volume_and_no_inverted_element() {
    let s = mesh_stats(&read_msh(TET10).unwrap()).unwrap();
    assert_eq!(s.types, [("C3D10".to_owned(), 1)]);
    assert!(s.volume > 0.0 && s.min_scaled_jacobian > 0.0);
    assert_eq!(s.inverted, 0);
}

#[test]
fn a_mirrored_hex_is_inverted() {
    let mut m = read_msh(HEX).unwrap();
    for n in &mut m.nodes {
        n.xyz[2] = -n.xyz[2];
    }
    let s = mesh_stats(&m).unwrap();
    assert_eq!(s.inverted, 1);
    assert_eq!(s.inverted_ids, [m.elements[0].id]);
    assert!(s.min_scaled_jacobian < 0.0 && s.volume < 0.0);
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
fn a_folded_hex_is_inverted_at_one_corner() {
    // Move one top corner below the bottom face: the corner Jacobian turns negative.
    let mut m = read_msh(HEX).unwrap();
    let top = m.elements[0].nodes[6];
    let node = m.nodes.iter_mut().find(|n| n.id == top).unwrap();
    node.xyz = [1.0, 1.0, -0.5];
    assert_eq!(mesh_stats(&m).unwrap().inverted, 1);
}

#[test]
fn a_missing_node_is_an_error() {
    let mut m = read_msh(HEX).unwrap();
    m.nodes.pop();
    assert!(mesh_stats(&m).unwrap_err().contains("missing node"));
}
