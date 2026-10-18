//! Element-face surfaces: a coupling that names an element surface uses the nodes of its faces,
//! and `*DSLOAD` puts a pressure on its faces.

mod common;

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
fn a_coupling_on_a_missing_surface_is_an_error() {
    let text = format!(
        "{CUBE}*SURFACE, TYPE=ELEMENT, NAME=T\nE, S4\n*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*KINEMATIC\n"
    );
    let e = parse(&text).unwrap_err();
    assert!(e.to_string().contains("surface 'S' is not defined"), "{e}");
}

#[test]
fn a_coupling_on_an_invalid_face_is_an_error() {
    let text = format!(
        "{CUBE}*SURFACE, TYPE=ELEMENT, NAME=S\nE, S9\n*COUPLING, CONSTRAINT NAME=C, REF NODE=9, SURFACE=S\n*KINEMATIC\n\
*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n*CLOAD\n9,1,100\n*END STEP\n"
    );
    match parse(&text) {
        Err(_) => {}
        Ok(m) => assert!(solve(&m).is_err()),
    }
}

/// `*DSLOAD` on an element surface is the same load as `*DLOAD` on the same faces.
#[test]
fn a_dsload_equals_a_dload_on_the_same_faces() {
    // The cube without the extra node 9. Face x = 0 slides, node 1 is pinned.
    let cube = CUBE.replace("9,3,0.5,0.5\n", "");
    let fixed = "*STEP\n*STATIC\n*BOUNDARY\n1,1,3\n4,1,1\n5,1,1\n8,1,1\n2,3,3\n3,3,3\n";
    let by_face = parse(&format!("{cube}{fixed}*DLOAD\nE, P4, -10\n*END STEP\n")).unwrap();
    let by_surface = parse(&format!(
        "{cube}*SURFACE, TYPE=ELEMENT, NAME=S\nE, S4\n{fixed}*DSLOAD\nS, P, -10\n*END STEP\n"
    ))
    .unwrap();
    let a = solve(&by_face).unwrap();
    let b = solve(&by_surface).unwrap();
    assert_eq!(a.steps[0].displacement, b.steps[0].displacement);
    // Negative pressure pulls the face x = 1 along +x, so the test is not empty.
    assert!(b.steps[0].displacement[6][0] > 0.0);
}

/// The faces (element, face number) of a tet10 block whose corners all pass `on`.
/// TET_FACES order: face 1 = 0 1 2, face 2 = 0 3 1, face 3 = 1 3 2, face 4 = 2 3 0.
fn tet_faces(b: &common::Block, on: impl Fn([f64; 3]) -> bool) -> Vec<(usize, u8)> {
    const CORNERS: [[usize; 3]; 4] = [[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]];
    let mut out = Vec::new();
    for (e, nodes) in b.elements.iter().enumerate() {
        for (f, c) in CORNERS.iter().enumerate() {
            if c.iter().all(|&l| on(b.nodes[nodes[l]])) {
                out.push((e + 1, f as u8 + 1));
            }
        }
    }
    out
}

fn surface_text(name: &str, faces: &[(usize, u8)]) -> String {
    let mut s = format!("*SURFACE, TYPE=ELEMENT, NAME={name}\n");
    for (e, f) in faces {
        s.push_str(&format!("{e}, S{f}\n"));
    }
    s
}

fn node_lines(b: &common::Block, on: impl Fn([f64; 3]) -> bool, dofs: &str) -> String {
    let mut s = String::new();
    for (i, p) in b.nodes.iter().enumerate() {
        if on(*p) {
            s.push_str(&format!("{}, {dofs}\n", i + 1));
        }
    }
    s
}

/// A pull of 10 on the face x = 2 of a distorted C3D10 block. The exact field is linear,
/// so C3D10 reproduces it to round-off at every node.
#[test]
fn a_dsload_on_a_tet10_face_gives_the_exact_tension() {
    let b = common::tet10_block([2, 2, 2], [2.0, 1.0, 1.0], 0.1);
    let tip = tet_faces(&b, |p| (p[0] - 2.0).abs() < 1e-12);
    // 2 x 2 cells on the face, 2 triangles each.
    assert_eq!(tip.len(), 8);
    let mut tail = surface_text("TIP", &tip);
    tail.push_str("*STEP\n*STATIC\n*BOUNDARY\n");
    tail.push_str(&node_lines(&b, |p| p[0].abs() < 1e-12, "1, 1"));
    tail.push_str(&node_lines(&b, |p| p[1].abs() < 1e-12, "2, 2"));
    tail.push_str(&node_lines(&b, |p| p[2].abs() < 1e-12, "3, 3"));
    tail.push_str("*DSLOAD\nTIP, P, -10\n*END STEP\n");
    let model = common::model(&b, &tail);
    let sol = solve(&model).unwrap();
    // E = 1000, nu = 0.3, sigma_xx = 10: strain 0.01, lateral strain -0.003.
    for (i, p) in b.nodes.iter().enumerate() {
        let u = sol.steps[0].displacement[i];
        let exact = [0.01 * p[0], -0.003 * p[1], -0.003 * p[2]];
        for k in 0..3 {
            assert!((u[k] - exact[k]).abs() < 1e-12, "node {} {u:?} {exact:?}", i + 1);
        }
    }
    // The reactions on x = 0 balance the load: pressure 10 times area 1.
    let rf: f64 = b
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, p)| p[0].abs() < 1e-12)
        .map(|(i, _)| sol.steps[0].reaction[i][0])
        .sum();
    assert!((rf + 10.0).abs() < 1e-9, "reaction {rf}");
}

/// A quarter of a thick cylinder, inner radius 1, outer radius 2, under an inner pressure of 10,
/// in plane strain. The quadratic faces of the bore are curved.
/// The x force of the pressure is the pressure times the projected area (radius times length)
/// for any mesh, because the edges of the bore lie on the planes x = 0, y = 0 and z = 0, L.
/// So the reactions balance it to round-off. The Lame solution checks the radial displacement.
#[test]
fn a_dsload_on_a_curved_tet10_bore_balances_the_reactions() {
    let (a, r_out, len, p) = (1.0, 2.0, 0.25, 10.0);
    let mut b = common::tet10_block([4, 8, 1], [1.0, 1.0, len], 0.0);
    let bore = tet_faces(&b, |q| q[0].abs() < 1e-12);
    assert_eq!(bore.len(), 16);
    // (s, t, z) to (r cos theta, r sin theta, z). The midside nodes go onto the curved edges.
    let params = b.nodes.clone();
    for (n, q) in b.nodes.iter_mut().zip(&params) {
        let r = a + (r_out - a) * q[0];
        let th = std::f64::consts::FRAC_PI_2 * q[1];
        *n = [r * th.cos(), r * th.sin(), q[2]];
    }
    let mut tail = surface_text("BORE", &bore);
    tail.push_str("*STEP\n*STATIC\n*BOUNDARY\n");
    for (i, q) in params.iter().enumerate() {
        if q[1].abs() < 1e-12 {
            tail.push_str(&format!("{}, 2, 2\n", i + 1));
        }
        if (q[1] - 1.0).abs() < 1e-12 {
            tail.push_str(&format!("{}, 1, 1\n", i + 1));
        }
        if q[2].abs() < 1e-12 || (q[2] - len).abs() < 1e-12 {
            tail.push_str(&format!("{}, 3, 3\n", i + 1));
        }
    }
    tail.push_str(&format!("*DSLOAD\nBORE, P, {p}\n*END STEP\n"));
    let model = common::model(&b, &tail);
    let sol = solve(&model).unwrap();
    let step = &sol.steps[0];
    let sum = |plane: f64, dof: usize| -> f64 {
        (0..params.len())
            .filter(|&i| (params[i][1] - plane).abs() < 1e-12)
            .map(|i| step.reaction[i][dof])
            .sum()
    };
    // The plane x = 0 is t = 1. The plane y = 0 is t = 0.
    let (rf_x, rf_y) = (sum(1.0, 0), sum(0.0, 1));
    let force = p * a * len;
    assert!((rf_x + force).abs() < 1e-9 * force, "rf_x {rf_x}, load {force}");
    assert!((rf_y + force).abs() < 1e-9 * force, "rf_y {rf_y}, load {force}");
    // Lame, plane strain: u_r(a) = (1 + nu) / E * ((1 - 2 nu) A a + B / a),
    // A = p a^2 / (b^2 - a^2), B = p a^2 b^2 / (b^2 - a^2). E = 1000, nu = 0.3.
    let (e, nu) = (1000.0, 0.3);
    let big_a = p * a * a / (r_out * r_out - a * a);
    let big_b = big_a * r_out * r_out;
    let exact = (1.0 + nu) / e * ((1.0 - 2.0 * nu) * big_a * a + big_b / a);
    for (i, q) in params.iter().enumerate() {
        if q[0].abs() < 1e-12 {
            let n = b.nodes[i];
            let u = step.displacement[i];
            let ur = (u[0] * n[0] + u[1] * n[1]) / a;
            // 4 x 8 quadratic cells: the discretisation error of u_r is below 1 percent.
            assert!((ur - exact).abs() < 0.01 * exact, "node {} u_r {ur}, exact {exact}", i + 1);
        }
    }
}
