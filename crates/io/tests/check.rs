use fh_io::{mesh_stats, read_msh};

const TET10: &str = include_str!("data/tet10.msh");

#[test]
fn tet10_has_a_positive_volume_and_no_inverted_element() {
    let s = mesh_stats(&read_msh(TET10).unwrap()).unwrap();
    assert_eq!(s.types, [("C3D10".to_owned(), 1)]);
    assert_eq!((s.bbox_min, s.bbox_max), ([0.0; 3], [1.0; 3]));
    assert!((s.volume - 1.0 / 6.0).abs() < 1e-12, "{}", s.volume);
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
