//! From gmsh to a solution: a block of 4 x 1 x 1 that gmsh meshes with extruded hexahedra
//! (solved as C3D8) and with quadratic tetrahedra (C3D10). The test imports the .msh file,
//! adds the material, the supports and the loads, solves, and compares the displacement of
//! every node with the exact answer.
//!
//! Both answers are polynomials that the elements hold exactly: a linear field (axial pull)
//! for C3D8 and C3D10, and a quadratic field (pure bending) for C3D10. Thus the only error is
//! rounding, and the tolerance is `TOL` of the largest displacement.
//!
//! Gmsh stays an external program (decision D-02). One test solves the stored meshes in
//! `fixtures/msh`. The other test runs gmsh on the .geo scripts first, and it skips with a
//! message when gmsh is not installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use fh_core_solver::solve;
use fh_inp::Model;
use fh_io::read_msh;

const L: f64 = 4.0;
const H: f64 = 1.0;
const W: f64 = 1.0;
const E: f64 = 1000.0;

/// Relative tolerance. It covers only rounding: on the development machine the error is
/// below 1e-13 of the largest displacement. The margin allows other machines and compilers.
const TOL: f64 = 1e-9;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/msh")
        .join(name)
}

/// The mesh, its nodes, elements and sets, as .inp text. Each element gets `element_type`.
fn inp_text(m: &Model, element_type: &str) -> String {
    let rows = |ids: &[u32]| {
        ids.chunks(16)
            .map(|c| c.iter().map(u32::to_string).collect::<Vec<_>>().join(", ") + "\n")
            .collect::<String>()
    };
    let mut t = String::from("*NODE\n");
    for n in &m.nodes {
        t += &format!("{}, {:?}, {:?}, {:?}\n", n.id, n.xyz[0], n.xyz[1], n.xyz[2]);
    }
    t += &format!("*ELEMENT, TYPE={element_type}\n");
    for e in &m.elements {
        let mut ids = vec![e.id];
        ids.extend(&e.nodes);
        t += &rows(&ids);
    }
    for s in &m.nsets {
        t += &format!("*NSET, NSET={}\n{}", s.name, rows(&s.ids));
    }
    for s in &m.elsets {
        t += &format!("*ELSET, ELSET={}\n{}", s.name, rows(&s.ids));
    }
    t
}

/// Import `msh`, check the element type and the sets that the .geo script names, and
/// return the model.
fn import(msh: &str, element_type: &str) -> Model {
    let m = read_msh(msh).unwrap_or_else(|e| panic!("{e}"));
    assert!(!m.elements.is_empty());
    assert!(m.elements.iter().all(|e| e.element_type == element_type));
    for name in ["FIX", "TIP", "BLOCK"] {
        assert!(m.nsets.iter().any(|s| s.name == name), "node set {name}");
    }
    assert!(m.elsets.iter().any(|s| s.name == "BLOCK"));
    m
}

const MATERIAL: &str = "*MATERIAL, NAME=M\n*ELASTIC\n1000., 0.\n\
*SOLID SECTION, ELSET=BLOCK, MATERIAL=M\n*BOUNDARY\nFIX, 1, 3\n";

/// Compare each node with `exact`. Returns the largest error, as a part of the largest
/// displacement.
fn compare(model: &Model, text: &str, exact: impl Fn([f64; 3]) -> [f64; 3]) -> f64 {
    let full = fh_inp::parse(text).unwrap_or_else(|e| panic!("{e}"));
    let s = solve(&full).unwrap_or_else(|e| panic!("{e}"));
    let u = &s.steps.last().unwrap().displacement;
    let (mut err, mut max) = (0.0f64, 0.0f64);
    for (n, ui) in full.nodes.iter().zip(u) {
        if !model.nodes.iter().any(|m| m.id == n.id) {
            continue;
        }
        let want = exact(n.xyz);
        for d in 0..3 {
            err = err.max((ui[d] - want[d]).abs());
            max = max.max(want[d].abs());
        }
    }
    err / max
}

/// The face x = 0 is fixed. The face x = L follows a reference node through a kinematic
/// coupling, and a force F pulls the reference node along x. With Poisson's ratio 0 the
/// exact answer is u_x = F x / (E W H), u_y = u_z = 0, and the tip moves F L / (E W H).
fn axial(model: &Model, element_type: &str) -> f64 {
    let f = 10.0;
    let reference = model.nodes.iter().map(|n| n.id).max().unwrap() + 1;
    let text = format!(
        "{}*NODE\n{reference}, {L}, {}, {}\n{MATERIAL}\
*SURFACE, TYPE=NODE, NAME=TIP_FACE\nTIP\n\
*COUPLING, CONSTRAINT NAME=PULL, REF NODE={reference}, SURFACE=TIP_FACE\n*KINEMATIC\n\
*STEP\n*STATIC\n*CLOAD\n{reference}, 1, {f}\n*END STEP\n",
        inp_text(model, element_type),
        H / 2.0,
        W / 2.0
    );
    let k = f / (E * W * H);
    assert!(
        (k * L - 0.04).abs() < 1e-15,
        "tip displacement F L / (E A) = 0.04"
    );
    compare(model, &text, |p| [k * p[0], 0.0, 0.0])
}

/// Pure bending: the face x = 0 is fixed, and each node of the face x = L gets
/// u_x = -k L (y - H / 2). With Poisson's ratio 0 the exact answer is
/// u_x = -k x (y - H / 2), u_y = k x^2 / 2, u_z = 0: the tip moves k L^2 / 2, as in beam
/// theory. The field is quadratic, so C3D10 holds it exactly.
fn bending(model: &Model) -> f64 {
    let k = 0.01;
    let tip = &model.nsets.iter().find(|s| s.name == "TIP").unwrap().ids;
    let mut text = format!(
        "{}{MATERIAL}*STEP\n*STATIC\n*BOUNDARY\n",
        inp_text(model, "C3D10")
    );
    for id in tip {
        let p = model.nodes.iter().find(|n| n.id == *id).unwrap().xyz;
        assert!((p[0] - L).abs() < 1e-12, "node {id} of TIP is at x = L");
        text += &format!("{id}, 1, 1, {:?}\n", -k * L * (p[1] - H / 2.0));
    }
    text += "*END STEP\n";
    assert!(
        (k * L * L / 2.0 - 0.08).abs() < 1e-15,
        "tip deflection k L^2 / 2 = 0.08"
    );
    compare(model, &text, |p| {
        [-k * p[0] * (p[1] - H / 2.0), k * p[0] * p[0] / 2.0, 0.0]
    })
}

/// Solve both meshes and check the answers. The .msh texts come from gmsh.
fn check(hex_msh: &str, tet10_msh: &str) {
    let hex = import(hex_msh, "C3D8I");
    assert_eq!(hex.elements.len(), 8 * 2 * 2);
    let tet = import(tet10_msh, "C3D10");
    let errors = [
        ("C3D8 axial", axial(&hex, "C3D8")),
        ("C3D10 axial", axial(&tet, "C3D10")),
        ("C3D10 bending", bending(&tet)),
    ];
    eprintln!(
        "{} hexahedra, {} tetrahedra; error / largest displacement: {errors:?}",
        hex.elements.len(),
        tet.elements.len()
    );
    for (case, err) in errors {
        assert!(err <= TOL, "{case}: error {err:e} > {TOL:e}");
    }
}

#[test]
fn the_stored_gmsh_meshes_give_the_exact_answers() {
    let read = |name: &str| std::fs::read_to_string(fixture(name)).unwrap();
    check(&read("block-hex.msh"), &read("block-tet10.msh"));
}

/// The gmsh program: `FH_GMSH` when it is set, else `gmsh` on the PATH or in the usual
/// install places.
fn gmsh() -> Option<PathBuf> {
    let list: Vec<PathBuf> = match std::env::var_os("FH_GMSH") {
        Some(p) => vec![PathBuf::from(p)],
        None => ["gmsh", "/opt/homebrew/bin/gmsh", "/usr/local/bin/gmsh"]
            .map(PathBuf::from)
            .to_vec(),
    };
    list.into_iter().find(|p| {
        Command::new(p)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

#[test]
fn gmsh_meshes_the_block_and_the_solver_gives_the_exact_answers() {
    let Some(gmsh) = gmsh() else {
        eprintln!(
            "SKIPPED: gmsh is not installed. Install gmsh 4, or set FH_GMSH to the gmsh \
program, to run this test. The stored meshes in fixtures/msh are still solved."
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!("fh-gmsh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mesh = |name: &str| {
        let out = dir.join(format!("{name}.msh"));
        let status = Command::new(&gmsh)
            .arg(fixture(&format!("{name}.geo")))
            .args(["-3", "-format", "msh41", "-v", "0", "-o"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success(), "gmsh failed on {name}.geo");
        std::fs::read_to_string(out).unwrap()
    };
    eprintln!("gmsh: {}", gmsh.display());
    check(&mesh("block-hex"), &mesh("block-tet10"));
}
