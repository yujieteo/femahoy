//! Structured block meshes for the tests. No mesher is part of the product.

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
