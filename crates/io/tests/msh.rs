use fh_io::read_msh;

const TET10: &str = include_str!("data/tet10.msh");

fn err(text: &str) -> String {
    read_msh(text).unwrap_err().to_string()
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
    assert!(m.elset("fixed").is_none());
}

#[test]
fn errors_name_the_line() {
    let old = TET10.replace("4.1 0 8", "2.2 0 8");
    assert_eq!(
        err(&old),
        "line 2: unsupported MSH version 2.2, expected 4.1 (MSH002)"
    );
    let bin = TET10.replace("4.1 0 8", "4.1 1 8");
    assert_eq!(
        err(&bin),
        "line 2: binary MSH is not supported, write ASCII (MSH002)"
    );
    let tet4 = TET10
        .replace("3 1 11 1", "3 1 4 1")
        .replace(" 5 6 7 8 9 10\n$EndElements", "\n$EndElements");
    assert_eq!(
        err(&tet4),
        "line 42: element type 4 is not accepted, use 11 (C3D10) (MSH002)"
    );
    let hex = TET10
        .replace("3 1 11 1", "3 1 5 1")
        .replace(" 9 10\n$EndElements", "\n$EndElements");
    assert_eq!(
        err(&hex),
        "line 42: element type 5 is not accepted, use 11 (C3D10) (MSH002)"
    );
    let bad_node = TET10.replace("2 1 2 3 4 5 6 7 8 9 10", "2 1 2 3 4 5 6 7 8 9 99");
    assert_eq!(
        err(&bad_node),
        "line 43: element 2 uses unknown node 99 (MSH004)"
    );
    let short = TET10.replace("2 1 2 3 4 5 6 7 8 9 10", "2 1 2 3 4 5 6 7 8 9");
    assert_eq!(
        err(&short),
        "line 43: element type 11 needs 10 nodes, found 9 (MSH001)"
    );
    let junk = TET10.replace("0 0 0\n1 0 0", "0 0 x\n1 0 0");
    assert!(err(&junk).starts_with("line 27: "));
    assert!(err("").starts_with("line 1: "));
}

#[test]
fn a_coordinate_that_is_not_finite_is_an_error() {
    let mut bad = Vec::new();
    for xyz in ["NaN 0 0", "0 inf 0", "0 0 -inf", "1e999 0 0"] {
        let text = TET10.replace("0 0 0\n1 0 0", &format!("{xyz}\n1 0 0"));
        assert_ne!(text, TET10);
        match read_msh(&text) {
            Ok(m) => bad.push(format!("{xyz}: accepted as {:?}", m.nodes[0].xyz)),
            Err(e) if !e.to_string().starts_with("line 27: ") => {
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
        ("3 1 0 10\n1\n", "3 1 0 10\n0\n", "line 17: "),
        ("2 1 2 3 4 5 6 7 8 9 10", "0 1 2 3 4 5 6 7 8 9 10", "line 43: "),
    ] {
        let text = TET10.replace(from, to);
        assert_ne!(text, TET10);
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
    // The triangle 1 2 3 is face S1 (corners 1 2 3) of the tet. A volume group gives no
    // element surface.
    let tet = fh_io::read_msh_mesh(TET10).unwrap();
    assert_eq!(tet.model.element_surface("fixed").unwrap().faces, [(2, 1)]);
    assert!(tet.model.element_surface("body").is_none());
    assert!(tet.unnamed_volumes.is_empty() && tet.inner_surfaces.is_empty());
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
