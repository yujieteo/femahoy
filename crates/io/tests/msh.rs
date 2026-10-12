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
        "line 2: unsupported MSH version 2.2, expected 4.1"
    );
    let bin = HEX.replace("4.1 0 8", "4.1 1 8");
    assert_eq!(
        err(&bin),
        "line 2: binary MSH is not supported, write ASCII"
    );
    let tet4 = TET10
        .replace("3 1 11 1", "3 1 4 1")
        .replace(" 5 6 7 8 9 10\n$EndElements", "\n$EndElements");
    assert_eq!(
        err(&tet4),
        "line 42: element type 4 is not accepted, use 5 or 11"
    );
    let bad_node = HEX.replace("3 1 2 3 4 5 6 7 8", "3 1 2 3 4 5 6 7 99");
    assert_eq!(err(&bad_node), "line 45: element 3 uses unknown node 99");
    let short = HEX.replace("3 1 2 3 4 5 6 7 8", "3 1 2 3 4 5 6 7");
    assert_eq!(
        err(&short),
        "line 45: element type 5 needs 8 nodes, found 7"
    );
    let junk = HEX.replace("0 0 0\n2 1 0 3", "0 0 x\n2 1 0 3");
    assert!(err(&junk).starts_with("line 20: "));
    assert!(err("").starts_with("line 1: "));
}
