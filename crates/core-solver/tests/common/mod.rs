//! Structured block meshes for the tests. No mesher is part of the product.
// Each test binary uses only some of the helpers.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use fh_inp::{parse, Model};

pub struct Block {
    pub ty: &'static str,
    pub nodes: Vec<[f64; 3]>,
    /// Zero-based node indices of each element, in the element node order.
    pub elements: Vec<Vec<usize>>,
}

/// A point grid with `m` cells per side. A smooth map moves the points inside.
/// The map vanishes on the faces, so the block keeps its shape.
fn lattice(m: [usize; 3], size: [f64; 3], jitter: f64) -> Vec<[f64; 3]> {
    let pi = std::f64::consts::PI;
    let weight = [1.0, 0.7, -0.5];
    let mut pts = Vec::new();
    for k in 0..=m[2] {
        for j in 0..=m[1] {
            for i in 0..=m[0] {
                let t = [
                    i as f64 / m[0] as f64,
                    j as f64 / m[1] as f64,
                    k as f64 / m[2] as f64,
                ];
                let bump = (pi * t[0]).sin() * (pi * t[1]).sin() * (pi * t[2]).sin();
                let mut p = [0.0; 3];
                for d in 0..3 {
                    p[d] = size[d] * (t[d] + jitter * weight[d] * bump);
                }
                pts.push(p);
            }
        }
    }
    pts
}

fn at(m: [usize; 3], i: usize, j: usize, k: usize) -> usize {
    (k * (m[1] + 1) + j) * (m[0] + 1) + i
}

#[allow(dead_code)]
pub fn hex_block(n: [usize; 3], size: [f64; 3], jitter: f64, ty: &'static str) -> Block {
    let mut elements = Vec::new();
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                let c = |a: usize, b: usize, d: usize| at(n, i + a, j + b, k + d);
                elements.push(vec![
                    c(0, 0, 0),
                    c(1, 0, 0),
                    c(1, 1, 0),
                    c(0, 1, 0),
                    c(0, 0, 1),
                    c(1, 0, 1),
                    c(1, 1, 1),
                    c(0, 1, 1),
                ]);
            }
        }
    }
    Block {
        ty,
        nodes: lattice(n, size, jitter),
        elements,
    }
}

/// Each cube becomes 6 tets around its main diagonal. Midside nodes sit on straight edges.
pub fn tet10_block(n: [usize; 3], size: [f64; 3], jitter: f64) -> Block {
    let mut nodes = lattice(n, size, jitter);
    let perms = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut edges: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut elements = Vec::new();
    for k in 0..n[2] {
        for j in 0..n[1] {
            for i in 0..n[0] {
                for p in perms {
                    let mut c = [[i, j, k]; 4];
                    for s in 0..3 {
                        c[s + 1] = c[s];
                        c[s + 1][p[s]] += 1;
                    }
                    let mut q: Vec<usize> = c.iter().map(|c| at(n, c[0], c[1], c[2])).collect();
                    let sub = |x: [f64; 3], y: [f64; 3]| [x[0] - y[0], x[1] - y[1], x[2] - y[2]];
                    let (r1, r2, r3) = (
                        sub(nodes[q[1]], nodes[q[0]]),
                        sub(nodes[q[2]], nodes[q[0]]),
                        sub(nodes[q[3]], nodes[q[0]]),
                    );
                    let det = r1[0] * (r2[1] * r3[2] - r2[2] * r3[1])
                        - r1[1] * (r2[0] * r3[2] - r2[2] * r3[0])
                        + r1[2] * (r2[0] * r3[1] - r2[1] * r3[0]);
                    if det < 0.0 {
                        q.swap(2, 3);
                    }
                    let mut mid = |x: usize, y: usize| {
                        let key = (x.min(y), x.max(y));
                        if let Some(&id) = edges.get(&key) {
                            return id;
                        }
                        let (a, b) = (nodes[x], nodes[y]);
                        nodes.push([
                            0.5 * (a[0] + b[0]),
                            0.5 * (a[1] + b[1]),
                            0.5 * (a[2] + b[2]),
                        ]);
                        edges.insert(key, nodes.len() - 1);
                        nodes.len() - 1
                    };
                    let mids = [
                        mid(q[0], q[1]),
                        mid(q[1], q[2]),
                        mid(q[2], q[0]),
                        mid(q[0], q[3]),
                        mid(q[1], q[3]),
                        mid(q[2], q[3]),
                    ];
                    q.extend(mids);
                    elements.push(q);
                }
            }
        }
    }
    Block {
        ty: "C3D10",
        nodes,
        elements,
    }
}

/// `.inp` text for the block with `E = 1000`, `nu = 0.3`. `tail` follows the section.
pub fn inp_text(b: &Block, tail: &str) -> String {
    let mut s = String::from("*NODE\n");
    for (i, p) in b.nodes.iter().enumerate() {
        s.push_str(&format!("{}, {:e}, {:e}, {:e}\n", i + 1, p[0], p[1], p[2]));
    }
    s.push_str(&format!("*ELEMENT, TYPE={}\n", b.ty));
    for (i, e) in b.elements.iter().enumerate() {
        let ids: Vec<String> = e.iter().map(|n| (n + 1).to_string()).collect();
        s.push_str(&format!("{}, {}\n", i + 1, ids.join(", ")));
    }
    s.push_str(&format!(
        "*ELSET, ELSET=ALL, GENERATE\n1, {}, 1\n*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.3\n*SOLID SECTION, ELSET=ALL, MATERIAL=M\n{tail}",
        b.elements.len()
    ));
    s
}

pub fn model(b: &Block, tail: &str) -> Model {
    parse(&inp_text(b, tail)).unwrap_or_else(|e| panic!("{e}"))
}

/// A C3D8 block with `steps` steps: the face x = 0 is fixed and the face x = size[0] has a
/// load that grows from step to step.
pub fn block_input(n: [usize; 3], size: [f64; 3], steps: usize) -> String {
    let b = hex_block(n, size, 0.1, "C3D8");
    let mut t = String::from("*NODE\n");
    for (i, p) in b.nodes.iter().enumerate() {
        t.push_str(&format!("{},{},{},{}\n", i + 1, p[0], p[1], p[2]));
    }
    t.push_str("*ELEMENT, TYPE=C3D8\n");
    for (i, e) in b.elements.iter().enumerate() {
        let ids: Vec<String> = e.iter().map(|n| (n + 1).to_string()).collect();
        t.push_str(&format!("{},{}\n", i + 1, ids.join(",")));
    }
    t.push_str(&format!(
        "*ELSET, ELSET=ALL, GENERATE\n1,{},1\n*MATERIAL, NAME=M\n*ELASTIC\n1000, 0.25\n\
*SOLID SECTION, ELSET=ALL, MATERIAL=M\n",
        b.elements.len()
    ));
    let at = |axis: usize, v: f64| -> Vec<usize> {
        b.nodes
            .iter()
            .enumerate()
            .filter(|(_, p)| (p[axis] - v).abs() < 1e-12)
            .map(|(i, _)| i + 1)
            .collect()
    };
    for s in 0..steps {
        t.push_str("*STEP\n*STATIC\n*BOUNDARY\n");
        for n in at(0, 0.0) {
            t.push_str(&format!("{n},1,3\n"));
        }
        t.push_str("*CLOAD\n");
        for n in at(0, size[0]) {
            t.push_str(&format!("{n},1,{}\n", 0.1 * (s + 1) as f64));
        }
        t.push_str("*END STEP\n");
    }
    t
}

/// Every file of the folder except the restart files and the input. The wall time and the
/// run id (the folder name) of the summary differ from run to run, so they are left out.
pub fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            !n.starts_with("restart") && !n.ends_with(".inp")
        })
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let mut bytes = fs::read(e.path()).unwrap();
            if name == "summary.toon" {
                let text = String::from_utf8(bytes).unwrap();
                let kept: Vec<&str> = text
                    .lines()
                    .filter(|l| !l.contains("wall_s:") && !l.starts_with("run_id:"))
                    .collect();
                bytes = kept.join("\n").into_bytes();
            }
            (name, bytes)
        })
        .collect();
    v.sort();
    v
}

pub fn assert_same(a: &[(String, Vec<u8>)], b: &[(String, Vec<u8>)]) {
    let names = |v: &[(String, Vec<u8>)]| v.iter().map(|x| x.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(a), names(b));
    for (x, y) in a.iter().zip(b) {
        if x.0.ends_with(".toon") || x.0.ends_with(".pvd") {
            assert_eq!(
                String::from_utf8_lossy(&x.1),
                String::from_utf8_lossy(&y.1),
                "{} differs",
                x.0
            );
        }
        assert!(x.1 == y.1, "{} differs", x.0);
    }
}
