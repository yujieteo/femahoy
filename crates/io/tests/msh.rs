use fh_io::read_msh;

const HEX: &str = include_str!("data/hex.msh");
const TET10: &str = include_str!("data/tet10.msh");

fn err(text: &str) -> String {
    read_msh(text).unwrap_err().to_string()
}

#[test]
fn hex_nodes_elements_and_sets() {
    let m = read_msh(HEX).unwrap();
    assert_eq!(m.nodes.len(), 8);
    let n7 = m.nodes.iter().find(|n| n.id == 7).unwrap();
    assert_eq!(n7.xyz, [1.0, 1.0, 1.0]);
    assert_eq!(m.elements.len(), 1);
    assert_eq!(m.elements[0].element_type, "C3D8I");
    assert_eq!(m.elements[0].nodes, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(m.nset("origin").unwrap().ids, [1]);
    assert_eq!(m.nset("xmin").unwrap().ids, [1, 4, 5, 8]);
    assert_eq!(m.nset("solid").unwrap().ids, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(m.elset("solid").unwrap().ids, [3]);
    assert!(m.elset("xmin").is_none());
}

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
}

#[test]
fn errors_name_the_line() {
    let old = HEX.replace("4.1 0 8", "2.2 0 8");
    assert_eq!(
        err(&old),
        "line 2: unsupported MSH version 2.2, expected 4.1 (MSH002)"
    );
    let bin = HEX.replace("4.1 0 8", "4.1 1 8");
    assert_eq!(
        err(&bin),
        "line 2: binary MSH is not supported, write ASCII (MSH002)"
    );
    let tet4 = TET10
        .replace("3 1 11 1", "3 1 4 1")
        .replace(" 5 6 7 8 9 10\n$EndElements", "\n$EndElements");
    assert_eq!(
        err(&tet4),
        "line 42: element type 4 is not accepted, use 5 or 11 (MSH002)"
    );
    let bad_node = HEX.replace("3 1 2 3 4 5 6 7 8", "3 1 2 3 4 5 6 7 99");
    assert_eq!(
        err(&bad_node),
        "line 45: element 3 uses unknown node 99 (MSH004)"
    );
    let short = HEX.replace("3 1 2 3 4 5 6 7 8", "3 1 2 3 4 5 6 7");
    assert_eq!(
        err(&short),
        "line 45: element type 5 needs 8 nodes, found 7 (MSH001)"
    );
    let junk = HEX.replace("0 0 0\n2 1 0 3", "0 0 x\n2 1 0 3");
    assert!(err(&junk).starts_with("line 20: "));
    assert!(err("").starts_with("line 1: "));
}

#[test]
fn a_coordinate_that_is_not_finite_is_an_error() {
    let mut bad = Vec::new();
    for xyz in ["NaN 0 0", "0 inf 0", "0 0 -inf", "1e999 0 0"] {
        let text = HEX.replace("0 0 0\n2 1 0 3", &format!("{xyz}\n2 1 0 3"));
        assert_ne!(text, HEX);
        match read_msh(&text) {
            Ok(m) => bad.push(format!("{xyz}: accepted as {:?}", m.nodes[0].xyz)),
            Err(e) if !e.to_string().starts_with("line 20: ") => {
                bad.push(format!("{xyz}: {e}"));
            }
            Err(_) => {}
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_tag_of_zero_is_an_error() {
    // Node and element tags become the labels of the model, and a label starts at 1.
    let mut bad = Vec::new();
    for (from, to, line) in [
        ("0 1 0 1\n1\n0 0 0", "0 1 0 1\n0\n0 0 0", "line 19: "),
        ("3 1 2 3 4 5 6 7 8", "0 1 2 3 4 5 6 7 8", "line 45: "),
    ] {
        let text = HEX.replace(from, to);
        assert_ne!(text, HEX);
        match read_msh(&text) {
            Ok(_) => bad.push(format!("{to:?}: accepted")),
            Err(e) if !e.to_string().starts_with(line) => bad.push(format!("{to:?}: {e}")),
            Err(_) => {}
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn a_surface_group_becomes_an_element_surface() {
    // The triangle 1 2 3 is face S1 (corners 1 2 3) of the tet. The quadrangle 1 4 8 5 is
    // face S6 (corners 4 8 5 1) of the hex.
    let tet = fh_io::read_msh_mesh(TET10).unwrap();
    assert_eq!(tet.model.element_surface("fixed").unwrap().faces, [(2, 1)]);
    assert!(tet.model.element_surface("body").is_none());
    assert!(tet.unnamed_volumes.is_empty() && tet.inner_surfaces.is_empty());
    let hex = read_msh(HEX).unwrap();
    assert_eq!(hex.element_surface("xmin").unwrap().faces, [(3, 6)]);
    // A point group and a volume group give no element surface.
    assert!(hex.element_surface("origin").is_none());
    assert!(hex.element_surface("solid").is_none());
}

#[test]
fn a_surface_off_the_solid_keeps_its_nodes_but_is_reported() {
    // The midside nodes 5 6 7 make no face of the tet.
    let off = TET10.replace("1 1 2 3\n3 1 11 1", "1 5 6 7\n3 1 11 1");
    let m = fh_io::read_msh_mesh(&off).unwrap();
    assert_eq!(m.inner_surfaces, ["fixed"]);
    assert!(m.model.element_surface("fixed").is_none());
    assert_eq!(m.model.nset("fixed").unwrap().ids, [5, 6, 7]);
}

#[test]
fn a_volume_with_no_physical_group_is_reported() {
    let unnamed = TET10.replace("1 0 0 0 1 1 1 1 9 0", "1 0 0 0 1 1 1 0 0");
    let m = fh_io::read_msh_mesh(&unnamed).unwrap();
    assert_eq!(m.unnamed_volumes, [1]);
    assert!(m.model.elset("body").is_none());
    // With no physical group at all, gmsh writes every element, so nothing is lost.
    let none = unnamed.replace("1 0 0 0 1 1 0 1 7 0", "1 0 0 0 1 1 0 0 0");
    assert!(fh_io::read_msh_mesh(&none)
        .unwrap()
        .unnamed_volumes
        .is_empty());
}
