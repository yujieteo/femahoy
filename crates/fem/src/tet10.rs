//! The 10-node tetrahedron of linear elasticity with straight edges, and the quadratic mesh of a
//! mesh of 4-node tetrahedra.
//!
//! The quadratic mesh adds a point at the middle of each edge of the linear mesh. 2 tetrahedra
//! that share a face share its 3 edges, so they share its 6 points, and the quadratic mesh is
//! conforming when the linear mesh is. The edges stay straight, so the map from the volume
//! coordinates L1..L4 to x is affine, and the gradients of the volume coordinates are those of
//! the linear tetrahedron.
//!
//! The displacement is quadratic: N_i = L_i (2 L_i - 1) at a corner and N_ij = 4 L_i L_j at the
//! middle of the edge ij. The strain is linear in the cell, so a pure bending field is exact and
//! a thin part does not lock in bending, as a linear tetrahedron does (its strain is constant).
//! The stiffness integrand is quadratic, and the 4-point rule of degree 2 integrates it exactly.
//! The thermal strain is alpha (T - T_ref) on the normal strains, with the temperature linear
//! between the 4 corners (the field of the heat solve on the linear mesh), so its load
//! integrand is also quadratic and exact.
//!
//! The consistent mass is the integral of density N_i N_j. Each product is a polynomial of degree
//! 4 in the volume coordinates, and the integral of L1^a L2^b L3^c L4^d over the cell is
//! 6 V a! b! c! d! / (a + b + c + d + 3)!, so the entries are exact multiples of V / 420.
//!
//! The consistent load of a uniform force per volume is -V/20 at each corner and V/5 at each
//! edge point, and that of a uniform force per area on a flat 6-point triangle is 0 at each
//! corner and A/3 at each edge point.
//!
//! Sources: O. C. Zienkiewicz, R. L. Taylor and J. Z. Zhu, The Finite Element Method: Its Basis
//! and Fundamentals, 7th ed., Elsevier 2013, sections 6.3 (volume coordinates and the quadratic
//! tetrahedron) and 6.6 (the consistent loads); P. C. Hammer, O. J. Marlowe and A. H. Stroud,
//! Numerical integration over simplexes and cones, Math. Tables Aids Comput. 10 (1956) 130-137
//! (the 4-point rule of degree 2); T. J. R. Hughes, The Finite Element Method, Prentice-Hall
//! 1987, section 2.7 (the strain-displacement matrix of elasticity).

use std::collections::HashMap;

use crate::hex::Stiffness;

/// The 2 corners of each edge point, in the order of a VTK quadratic tetrahedron.
pub const EDGES: [[usize; 2]; 6] = [[0, 1], [1, 2], [0, 2], [0, 3], [1, 3], [2, 3]];

/// The 2 axes of a shear strain: yz, xz, xy, as in `hex`.
const SHEAR: [[usize; 2]; 3] = [[1, 2], [0, 2], [0, 1]];

/// The volume coordinates of the 4 points of the rule of degree 2, each with the weight 1/4.
fn gauss() -> [[f64; 4]; 4] {
    let a = 0.585_410_196_624_968_5;
    let b = 0.138_196_601_125_010_5;
    std::array::from_fn(|q| std::array::from_fn(|k| if k == q { a } else { b }))
}

/// The values of the 10 functions at the volume coordinates `l`.
pub fn shape(l: [f64; 4]) -> [f64; 10] {
    let mut n = [0.0; 10];
    for i in 0..4 {
        n[i] = l[i] * (2.0 * l[i] - 1.0);
    }
    for (k, [i, j]) in EDGES.iter().enumerate() {
        n[4 + k] = 4.0 * l[*i] * l[*j];
    }
    n
}

/// A 10-node tetrahedron: the gradients of its 4 volume coordinates and its volume.
#[derive(Debug, Clone, Copy)]
pub struct Tet10 {
    pub g: [[f64; 3]; 4],
    pub volume: f64,
}

impl Tet10 {
    /// The element of the 4 corners `p`, with a positive volume.
    pub fn new(p: [[f64; 3]; 4]) -> Tet10 {
        let (g, volume) = crate::heat::tet_gradients(p);
        Tet10 { g, volume }
    }

    /// The gradients of the 10 functions at the volume coordinates `l`.
    fn gradients(&self, l: [f64; 4]) -> [[f64; 3]; 10] {
        let g = &self.g;
        let mut out = [[0.0; 3]; 10];
        for i in 0..4 {
            out[i] = g[i].map(|v| (4.0 * l[i] - 1.0) * v);
        }
        for (k, [i, j]) in EDGES.iter().enumerate() {
            out[4 + k] = std::array::from_fn(|d| 4.0 * (l[*i] * g[*j][d] + l[*j] * g[*i][d]));
        }
        out
    }

    /// The stiffness of the 30 unknowns (3 node + component), row-major.
    pub fn stiffness(&self, d: &Stiffness) -> Vec<f64> {
        let mut k = vec![0.0; 900];
        let w = self.volume / 4.0;
        for l in gauss() {
            let g = self.gradients(l);
            for i in 0..10 {
                let gi = g[i];
                for j in 0..10 {
                    let gj = g[j];
                    for a in 0..3 {
                        for b in 0..3 {
                            let mut v = gi[a] * gj[b] * d.normal[a][b];
                            if a == b {
                                for (s, [p, q]) in SHEAR.iter().enumerate() {
                                    if *p == a {
                                        v += gi[*q] * gj[*q] * d.shear[s];
                                    } else if *q == a {
                                        v += gi[*p] * gj[*p] * d.shear[s];
                                    }
                                }
                            } else {
                                v += gi[b] * gj[a] * d.shear[3 - a - b];
                            }
                            k[(3 * i + a) * 30 + 3 * j + b] += w * v;
                        }
                    }
                }
            }
        }
        k
    }

    /// The load of the thermal strain of the corner temperature changes `t`: the integral of
    /// B^T D alpha (T - T_ref). `thermal` is D alpha.
    pub fn thermal_load(&self, thermal: [f64; 3], t: [f64; 4]) -> [f64; 30] {
        let mut f = [0.0; 30];
        let w = self.volume / 4.0;
        for l in gauss() {
            let g = self.gradients(l);
            let dt: f64 = (0..4).map(|c| l[c] * t[c]).sum();
            for i in 0..10 {
                for a in 0..3 {
                    f[3 * i + a] += w * g[i][a] * thermal[a] * dt;
                }
            }
        }
        f
    }

    /// The total strain (engineering shear) and the stress at the volume coordinates `l`, from
    /// the displacements `u` of the 10 points and the corner temperature changes `t`. Both are
    /// in the order xx, yy, zz, yz, xz, xy, as in `hex`.
    pub fn strain_stress(
        &self,
        d: &Stiffness,
        thermal: [f64; 3],
        l: [f64; 4],
        u: &[f64; 30],
        t: [f64; 4],
    ) -> ([f64; 6], [f64; 6]) {
        let g = self.gradients(l);
        let mut e = [0.0; 6];
        for (i, gi) in g.iter().enumerate() {
            let ui = [u[3 * i], u[3 * i + 1], u[3 * i + 2]];
            for a in 0..3 {
                e[a] += gi[a] * ui[a];
            }
            for (s, [a, b]) in SHEAR.iter().enumerate() {
                e[3 + s] += gi[*a] * ui[*b] + gi[*b] * ui[*a];
            }
        }
        let dt: f64 = (0..4).map(|c| l[c] * t[c]).sum();
        let mut s = d.stress(&e);
        for a in 0..3 {
            s[a] -= thermal[a] * dt;
        }
        (e, s)
    }

    /// The consistent mass of the 30 unknowns with the density `density` (row-major).
    pub fn mass(&self, density: f64) -> Vec<f64> {
        // The scalar mass in units of V / 420: a corner with itself 6 and with another corner 1;
        // a corner with an edge point -4 on its edge and -6 off it; an edge point with itself
        // 32, with an edge that shares a corner 16, and with the opposite edge 8.
        let scalar = |i: usize, j: usize| -> f64 {
            match (i < 4, j < 4) {
                (true, true) => {
                    if i == j {
                        6.0
                    } else {
                        1.0
                    }
                }
                (true, false) | (false, true) => {
                    let (c, e) = if i < 4 { (i, j - 4) } else { (j, i - 4) };
                    if EDGES[e].contains(&c) {
                        -4.0
                    } else {
                        -6.0
                    }
                }
                (false, false) => {
                    let (a, b) = (EDGES[i - 4], EDGES[j - 4]);
                    match a.iter().filter(|c| b.contains(c)).count() {
                        2 => 32.0,
                        1 => 16.0,
                        _ => 8.0,
                    }
                }
            }
        };
        let w = density * self.volume / 420.0;
        let mut m = vec![0.0; 900];
        for i in 0..10 {
            for j in 0..10 {
                let v = w * scalar(i, j);
                for a in 0..3 {
                    m[(3 * i + a) * 30 + 3 * j + a] = v;
                }
            }
        }
        m
    }
}

/// The volume coordinates of the 10 points of a tetrahedron.
pub fn node_coordinates() -> [[f64; 4]; 10] {
    std::array::from_fn(|n| {
        let mut l = [0.0; 4];
        if n < 4 {
            l[n] = 1.0;
        } else {
            let [i, j] = EDGES[n - 4];
            l[i] = 0.5;
            l[j] = 0.5;
        }
        l
    })
}

/// The quadratic mesh of a mesh of 4-node tetrahedra.
#[derive(Debug, Clone, PartialEq)]
pub struct Quadratic {
    /// The corners of the linear mesh, in its order, then the edge points.
    pub points: Vec<[f64; 3]>,
    /// The number of corners: point `corners + k` is the middle of `edges[k]`.
    pub corners: usize,
    pub edges: Vec<[u32; 2]>,
    /// The 10 points of each tetrahedron, in the order of a VTK quadratic tetrahedron.
    pub tets: Vec<[u32; 10]>,
}

impl Quadratic {
    /// The mesh with the edge points of the tetrahedra `tets` on the corners `points`.
    pub fn new(points: &[[f64; 3]], tets: &[[u32; 4]]) -> Quadratic {
        let corners = points.len();
        let mut of: HashMap<(u32, u32), u32> = HashMap::new();
        let mut edges: Vec<[u32; 2]> = Vec::new();
        let mut out = Vec::with_capacity(tets.len());
        for tet in tets {
            let mut t = [0u32; 10];
            t[..4].copy_from_slice(tet);
            for (k, [i, j]) in EDGES.iter().enumerate() {
                let (a, b) = (tet[*i].min(tet[*j]), tet[*i].max(tet[*j]));
                t[4 + k] = *of.entry((a, b)).or_insert_with(|| {
                    edges.push([a, b]);
                    (corners + edges.len() - 1) as u32
                });
            }
            out.push(t);
        }
        let mut all = points.to_vec();
        all.extend(edges.iter().map(|[a, b]| {
            let (p, q) = (points[*a as usize], points[*b as usize]);
            std::array::from_fn(|d| 0.5 * (p[d] + q[d]))
        }));
        Quadratic {
            points: all,
            corners,
            edges,
            tets: out,
        }
    }

    /// The 6 points of the face `nodes` (3 corners) of the tetrahedron `tet`: the corners, then
    /// the middles of the edges 01, 12 and 20, in the order of a VTK quadratic triangle.
    pub fn face(&self, tet: usize, nodes: [u32; 3]) -> [u32; 6] {
        let t = &self.tets[tet];
        let middle = |a: u32, b: u32| {
            EDGES
                .iter()
                .position(|[i, j]| (t[*i] == a && t[*j] == b) || (t[*i] == b && t[*j] == a))
                .map(|k| t[4 + k])
                .expect("the face is a face of the tetrahedron")
        };
        [
            nodes[0],
            nodes[1],
            nodes[2],
            middle(nodes[0], nodes[1]),
            middle(nodes[1], nodes[2]),
            middle(nodes[2], nodes[0]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each entry of the mass is the exact integral of rho N_i N_j: the polynomial in the volume
    /// coordinates, term by term, with 6 V a! b! c! d! / (a + b + c + d + 3)!.
    #[test]
    fn the_mass_is_the_exact_integral_of_the_functions() {
        let tet = Tet10::new([
            [0.0, 0.0, 0.0],
            [2.0, 0.1, 0.0],
            [0.3, 1.5, 0.2],
            [0.1, 0.4, 0.9],
        ]);
        let m = tet.mass(3.0);
        // Each function as terms (coefficient, exponents of L1..L4).
        let terms = |i: usize| -> Vec<(f64, [u32; 4])> {
            let unit = |k: usize, p: u32| std::array::from_fn(|c| if c == k { p } else { 0 });
            if i < 4 {
                vec![(2.0, unit(i, 2)), (-1.0, unit(i, 1))]
            } else {
                let [a, b] = EDGES[i - 4];
                vec![(4.0, std::array::from_fn(|c| u32::from(c == a || c == b)))]
            }
        };
        let fact = |n: u32| (1..=n).map(f64::from).product::<f64>();
        for i in 0..10 {
            for j in 0..10 {
                let mut exact = 0.0;
                for (ci, ei) in terms(i) {
                    for (cj, ej) in terms(j) {
                        let e: [u32; 4] = std::array::from_fn(|c| ei[c] + ej[c]);
                        let num: f64 = e.iter().map(|&p| fact(p)).product();
                        exact += ci * cj * 6.0 * tet.volume * num / fact(e.iter().sum::<u32>() + 3);
                    }
                }
                assert!(
                    (m[3 * i * 30 + 3 * j] - 3.0 * exact).abs() < 1e-12,
                    "{i} {j}"
                );
            }
        }
    }
}
