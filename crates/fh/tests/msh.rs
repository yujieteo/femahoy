use fh::msh::read_msh;

const TET10: &str = include_str!("data/tet10.msh");
const SHELL6: &str = include_str!("data/shell6.msh");

#[test]
fn tet10_midside_nodes_follow_the_inp_order() {
    let m = read_msh(TET10).unwrap();
    let e = &m.elements[0];
    assert_eq!(e.element_type, "C3D10");
    let xyz = |id: u32| m.nodes.iter().find(|n| n.id == id).unwrap().xyz;
    // Midside node k joins these corner pairs (0-based corners).
    let pairs = [(0, 1), (1, 2), (2, 3), (0, 3), (1, 3), (2, 3)];
    let pairs = [pairs[0], pairs[1], (2, 0), pairs[3], pairs[4], pairs[5]];
    for (k, (a, b)) in pairs.iter().enumerate() {
        let (pa, pb, pm) = (xyz(e.nodes[*a]), xyz(e.nodes[*b]), xyz(e.nodes[4 + k]));
        for i in 0..3 {
            assert_eq!(pm[i], (pa[i] + pb[i]) / 2.0, "midside {k} axis {i}");
        }
    }
    assert_eq!(e.nodes, [1, 2, 3, 4, 5, 6, 7, 8, 10, 9]);
    assert_eq!(m.nset("fixed").unwrap().ids, [1, 2, 3]);
    assert_eq!(m.elset("body").unwrap().ids, [2]);
    assert!(m.elset("fixed").is_none());
}

#[test]
fn a_surface_group_becomes_an_element_surface() {
    // The triangle 1 2 3 is face S1 (corners 1 2 3) of the tet. A volume group gives no
    // element surface.
    let tet = fh::msh::read_msh_mesh(TET10).unwrap();
    assert_eq!(tet.model.element_surface("fixed").unwrap().faces, [(2, 1)]);
    assert!(tet.model.element_surface("body").is_none());
    assert!(tet.unnamed_volumes.is_empty() && tet.inner_surfaces.is_empty());
}

#[test]
fn a_shell_mesh_has_s6_elements_and_edge_surfaces() {
    // Two triangles on the unit square. The edge of a shell is face S3 to S5, in the order of
    // its corners. The diagonal belongs to both triangles, so it is not on the outside.
    let m = fh::msh::read_msh_shell(SHELL6).unwrap();
    let types: Vec<&str> = m
        .model
        .elements
        .iter()
        .map(|e| e.element_type.as_str())
        .collect();
    assert_eq!(types, ["S6", "S6"]);
    assert_eq!(m.model.elements[1].nodes, [1, 3, 4, 7, 8, 9]);
    assert_eq!(m.model.elset("plate").unwrap().ids, [4, 5]);
    assert_eq!(m.model.nset("bottom").unwrap().ids, [1, 2, 5]);
    assert_eq!(m.model.element_surface("bottom").unwrap().faces, [(4, 3)]);
    assert_eq!(m.model.element_surface("left").unwrap().faces, [(5, 5)]);
    assert!(m.model.element_surface("diag").is_none());
    assert_eq!(m.inner_surfaces, ["diag"]);
}
