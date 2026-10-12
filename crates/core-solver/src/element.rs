//! Linear elastic solid elements: `C3D8`, `C3D8I` and `C3D10`.
//!
//! Strain order is `[xx, yy, zz, xy, yz, xz]` with engineering shear strains.
//! Element matrices are row-major with 3 dofs per node.

use crate::dense::{a_b, at_b, inv3, invert};
use crate::SolverError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Hex8 with B-bar (selective reduced integration of the volume strain).
    C3D8,
    /// Hex8 with incompatible modes.
    C3D8I,
    /// Tet10 with a 4-point rule.
    C3D10,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Kind> {
        match name {
            "C3D8" => Some(Kind::C3D8),
            "C3D8I" => Some(Kind::C3D8I),
            "C3D10" => Some(Kind::C3D10),
            _ => None,
        }
    }

    pub fn node_count(self) -> usize {
        match self {
            Kind::C3D8 | Kind::C3D8I => 8,
            Kind::C3D10 => 10,
        }
    }
}

/// Isotropic elasticity matrix.
pub fn elasticity(e: f64, nu: f64) -> [[f64; 6]; 6] {
    let lam = e * nu / ((1.0 + nu) * (1.0 - 2.0 * nu));
    let mu = e / (2.0 * (1.0 + nu));
    let mut d = [[0.0; 6]; 6];
    for i in 0..3 {
        d[i][..3].fill(lam);
        d[i][i] += 2.0 * mu;
        d[i + 3][i + 3] = mu;
    }
    d
}

/// One integration point: weight times Jacobian, strain-displacement matrix (6 x 3n)
/// and the incompatible-mode matrix (6 x 9) when the element has one.
struct Point {
    w: f64,
    b: Vec<f64>,
    g: Option<Vec<f64>>,
}

const HEX_XI: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

fn hex_dn(xi: [f64; 3]) -> [[f64; 3]; 8] {
    let mut dn = [[0.0; 3]; 8];
    for (a, s) in HEX_XI.iter().enumerate() {
        let f = [1.0 + s[0] * xi[0], 1.0 + s[1] * xi[1], 1.0 + s[2] * xi[2]];
        dn[a] = [
            0.125 * s[0] * f[1] * f[2],
            0.125 * f[0] * s[1] * f[2],
            0.125 * f[0] * f[1] * s[2],
        ];
    }
    dn
}

fn tet_dn(l: [f64; 4]) -> [[f64; 3]; 10] {
    // Parametric coordinates (r, s, t) = (l2, l3, l4), l1 = 1 - r - s - t.
    let dl = [
        [-1.0, -1.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let mut dn = [[0.0; 3]; 10];
    for i in 0..4 {
        for k in 0..3 {
            dn[i][k] = (4.0 * l[i] - 1.0) * dl[i][k];
        }
    }
    for (m, (i, j)) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)]
        .into_iter()
        .enumerate()
    {
        for k in 0..3 {
            dn[4 + m][k] = 4.0 * (l[i] * dl[j][k] + l[j] * dl[i][k]);
        }
    }
    dn
}

fn jacobian(coords: &[[f64; 3]], dn: &[[f64; 3]]) -> [[f64; 3]; 3] {
    let mut j = [[0.0; 3]; 3];
    for (x, d) in coords.iter().zip(dn) {
        for i in 0..3 {
            for k in 0..3 {
                j[i][k] += x[i] * d[k];
            }
        }
    }
    j
}

/// Cartesian derivatives `dN/dx` from natural derivatives and the inverse Jacobian.
fn cartesian(dn: &[[f64; 3]], jinv: &[[f64; 3]; 3]) -> Vec<[f64; 3]> {
    dn.iter()
        .map(|d| {
            let mut c = [0.0; 3];
            for (i, ci) in c.iter_mut().enumerate() {
                *ci = (0..3).map(|j| d[j] * jinv[j][i]).sum();
            }
            c
        })
        .collect()
}

/// Strain-displacement matrix, 6 x 3n.
fn b_matrix(dx: &[[f64; 3]]) -> Vec<f64> {
    let n = dx.len();
    let mut b = vec![0.0; 6 * 3 * n];
    for (a, d) in dx.iter().enumerate() {
        let c = 3 * a;
        let w = 3 * n;
        b[c] = d[0];
        b[w + c + 1] = d[1];
        b[2 * w + c + 2] = d[2];
        b[3 * w + c] = d[1];
        b[3 * w + c + 1] = d[0];
        b[4 * w + c + 1] = d[2];
        b[4 * w + c + 2] = d[1];
        b[5 * w + c] = d[2];
        b[5 * w + c + 2] = d[0];
    }
    b
}

fn check_det(det: f64, kind: Kind) -> Result<(), SolverError> {
    if det > 0.0 {
        Ok(())
    } else {
        Err(SolverError::new(format!(
            "{kind:?} element has a non-positive Jacobian ({det:e})"
        )))
    }
}

fn points(kind: Kind, coords: &[[f64; 3]]) -> Result<Vec<Point>, SolverError> {
    if coords.len() != kind.node_count() {
        return Err(SolverError::new(format!(
            "{kind:?} needs {} nodes, found {}",
            kind.node_count(),
            coords.len()
        )));
    }
    match kind {
        Kind::C3D10 => tet10_points(coords),
        Kind::C3D8 | Kind::C3D8I => hex8_points(kind, coords),
    }
}

fn tet10_points(coords: &[[f64; 3]]) -> Result<Vec<Point>, SolverError> {
    let (a, b) = (0.138_196_601_125_010_5, 0.585_410_196_624_968_5);
    let mut out = Vec::with_capacity(4);
    for p in 0..4 {
        let mut l = [a; 4];
        l[p] = b;
        let dn = tet_dn(l);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, Kind::C3D10)?;
        out.push(Point {
            w: det / 24.0,
            b: b_matrix(&cartesian(&dn, &jinv)),
            g: None,
        });
    }
    Ok(out)
}

fn hex8_points(kind: Kind, coords: &[[f64; 3]]) -> Result<Vec<Point>, SolverError> {
    let g = 1.0 / 3.0_f64.sqrt();
    let mut out = Vec::with_capacity(8);
    let (jinv0, det0) = inv3(&jacobian(coords, &hex_dn([0.0; 3])));
    check_det(det0, kind)?;
    for s in HEX_XI {
        let xi = [s[0] * g, s[1] * g, s[2] * g];
        let dn = hex_dn(xi);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, kind)?;
        let b = b_matrix(&cartesian(&dn, &jinv));
        let gm = (kind == Kind::C3D8I).then(|| {
            // Incompatible modes 1 - xi^2, 1 - eta^2, 1 - zeta^2: derivatives mapped
            // by the centre Jacobian and scaled by det0 / det. Column 3k + i is
            // mode k acting in direction i.
            let mut gm = vec![0.0; 6 * 9];
            for k in 0..3 {
                let mut dnk = [[0.0; 3]; 1];
                dnk[0][k] = -2.0 * xi[k];
                let d = cartesian(&dnk, &jinv0)[0].map(|v| v * det0 / det);
                let bk = b_matrix(&[d]);
                for r in 0..6 {
                    gm[r * 9 + 3 * k..r * 9 + 3 * k + 3].copy_from_slice(&bk[r * 3..r * 3 + 3]);
                }
            }
            gm
        });
        out.push(Point { w: det, b, g: gm });
    }
    if kind == Kind::C3D8 {
        bbar(&mut out);
    }
    Ok(out)
}

/// Replace the volume strain part of each `B` with its element average.
fn bbar(points: &mut [Point]) {
    let n3 = points[0].b.len() / 6;
    let vol: f64 = points.iter().map(|p| p.w).sum();
    let mut avg = vec![0.0; n3];
    for p in points.iter() {
        for (c, a) in avg.iter_mut().enumerate() {
            *a += p.w * (p.b[c] + p.b[n3 + c] + p.b[2 * n3 + c]);
        }
    }
    for a in &mut avg {
        *a /= vol;
    }
    for p in points.iter_mut() {
        for (c, a) in avg.iter().enumerate() {
            let local = p.b[c] + p.b[n3 + c] + p.b[2 * n3 + c];
            let delta = (a - local) / 3.0;
            for r in 0..3 {
                p.b[r * n3 + c] += delta;
            }
        }
    }
}

fn flat(d: &[[f64; 6]; 6]) -> Vec<f64> {
    d.iter().flatten().copied().collect()
}

/// Condensed element data: stiffness and the matrices that recover the
/// incompatible mode amplitudes.
struct Condensed {
    k: Vec<f64>,
    /// `alpha = recover * u` (9 x 3n), present for `C3D8I`.
    recover: Option<Vec<f64>>,
}

fn condense(
    kind: Kind,
    coords: &[[f64; 3]],
    d: &[[f64; 6]; 6],
) -> Result<(Vec<Point>, Condensed), SolverError> {
    let pts = points(kind, coords)?;
    let n3 = 3 * kind.node_count();
    let df = flat(d);
    let mut k = vec![0.0; n3 * n3];
    let (mut kua, mut kaa) = (vec![0.0; n3 * 9], vec![0.0; 81]);
    for p in &pts {
        let db = a_b(&df, &p.b, 6, 6, n3);
        for (x, v) in k.iter_mut().zip(at_b(&p.b, &db, 6, n3, n3)) {
            *x += p.w * v;
        }
        if let Some(g) = &p.g {
            let dg = a_b(&df, g, 6, 6, 9);
            for (x, v) in kua.iter_mut().zip(at_b(&p.b, &dg, 6, n3, 9)) {
                *x += p.w * v;
            }
            for (x, v) in kaa.iter_mut().zip(at_b(g, &dg, 6, 9, 9)) {
                *x += p.w * v;
            }
        }
    }
    if pts[0].g.is_none() {
        return Ok((pts, Condensed { k, recover: None }));
    }
    let kaa_inv =
        invert(&kaa, 9).ok_or_else(|| SolverError::new("incompatible mode matrix is singular"))?;
    // recover = -Kaa^-1 * Kau, and K = Kuu - Kua * Kaa^-1 * Kau.
    let mut kau = vec![0.0; 9 * n3];
    for i in 0..n3 {
        for j in 0..9 {
            kau[j * n3 + i] = kua[i * 9 + j];
        }
    }
    let mut recover = a_b(&kaa_inv, &kau, 9, 9, n3);
    for r in &mut recover {
        *r = -*r;
    }
    let corr = a_b(&kua, &recover, n3, 9, n3);
    for (x, c) in k.iter_mut().zip(corr) {
        *x += c;
    }
    Ok((
        pts,
        Condensed {
            k,
            recover: Some(recover),
        },
    ))
}

/// Element stiffness, `3n x 3n` row-major.
pub fn stiffness(
    kind: Kind,
    coords: &[[f64; 3]],
    d: &[[f64; 6]; 6],
) -> Result<Vec<f64>, SolverError> {
    Ok(condense(kind, coords, d)?.1.k)
}

/// Element volume.
pub fn volume(kind: Kind, coords: &[[f64; 3]]) -> Result<f64, SolverError> {
    Ok(points(kind, coords)?.iter().map(|p| p.w).sum())
}

/// Volume-averaged stress of the element for the nodal displacements `u`.
pub fn mean_stress(
    kind: Kind,
    coords: &[[f64; 3]],
    d: &[[f64; 6]; 6],
    u: &[f64],
) -> Result<[f64; 6], SolverError> {
    let (pts, c) = condense(kind, coords, d)?;
    let n3 = 3 * kind.node_count();
    let alpha = c.recover.map(|r| a_b(&r, u, 9, n3, 1));
    let mut s = [0.0; 6];
    let mut vol = 0.0;
    for p in &pts {
        let mut eps = a_b(&p.b, u, 6, n3, 1);
        if let (Some(g), Some(a)) = (&p.g, &alpha) {
            for (e, v) in eps.iter_mut().zip(a_b(g, a, 6, 9, 1)) {
                *e += v;
            }
        }
        for i in 0..6 {
            s[i] += p.w * (0..6).map(|j| d[i][j] * eps[j]).sum::<f64>();
        }
        vol += p.w;
    }
    Ok(s.map(|v| v / vol))
}

/// Von Mises equivalent stress from `[xx, yy, zz, xy, yz, xz]`.
pub fn von_mises(s: &[f64; 6]) -> f64 {
    let d = [s[0] - s[1], s[1] - s[2], s[2] - s[0]];
    (0.5 * (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
        + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5]))
        .sqrt()
}

/// Consistent nodal forces of a uniform pressure `p` on face `face` (1 to 6).
/// Positive pressure pushes into the body. Returns `(local node, force)` pairs.
pub fn pressure_load(
    kind: Kind,
    coords: &[[f64; 3]],
    face: u8,
    p: f64,
) -> Result<Vec<(usize, [f64; 3])>, SolverError> {
    let hex_faces: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [4, 7, 6, 5],
        [0, 4, 5, 1],
        [1, 5, 6, 2],
        [2, 6, 7, 3],
        [3, 7, 4, 0],
    ];
    // Tet faces: corners, then the three midside nodes in the same cyclic order.
    let tet_faces: [[usize; 6]; 4] = [
        [0, 1, 2, 4, 5, 6],
        [0, 3, 1, 7, 8, 4],
        [1, 3, 2, 8, 9, 5],
        [2, 3, 0, 9, 7, 6],
    ];
    let f = usize::from(face) - 1;
    let bad = || SolverError::new(format!("{kind:?} has no face {face}"));
    let mut out: Vec<(usize, [f64; 3])> = Vec::new();
    match kind {
        Kind::C3D8 | Kind::C3D8I => {
            let nodes = hex_faces.get(f).ok_or_else(bad)?;
            let g = 1.0 / 3.0_f64.sqrt();
            for (sa, sb) in [(-g, -g), (g, -g), (g, g), (-g, g)] {
                // Bilinear quad shape functions on the face.
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
                let mut n = [0.0; 4];
                let (mut ta, mut tb) = ([0.0; 3], [0.0; 3]);
                for (i, (ca, cb)) in corners.iter().enumerate() {
                    n[i] = 0.25 * (1.0 + ca * sa) * (1.0 + cb * sb);
                    let (da, db) = (0.25 * ca * (1.0 + cb * sb), 0.25 * cb * (1.0 + ca * sa));
                    for k in 0..3 {
                        ta[k] += da * coords[nodes[i]][k];
                        tb[k] += db * coords[nodes[i]][k];
                    }
                }
                let nrm = cross(ta, tb);
                for (i, &ln) in nodes.iter().enumerate() {
                    out.push((ln, nrm.map(|c| p * n[i] * c)));
                }
            }
        }
        Kind::C3D10 => {
            let nodes = tet_faces.get(f).ok_or_else(bad)?;
            // 3-point rule on the triangle, quadratic shape functions.
            for (l1, l2) in [
                (1.0 / 6.0, 1.0 / 6.0),
                (2.0 / 3.0, 1.0 / 6.0),
                (1.0 / 6.0, 2.0 / 3.0),
            ] {
                let l = [1.0 - l1 - l2, l1, l2];
                let n = [
                    l[0] * (2.0 * l[0] - 1.0),
                    l[1] * (2.0 * l[1] - 1.0),
                    l[2] * (2.0 * l[2] - 1.0),
                    4.0 * l[0] * l[1],
                    4.0 * l[1] * l[2],
                    4.0 * l[2] * l[0],
                ];
                let dl = [[-1.0, -1.0], [1.0, 0.0], [0.0, 1.0]];
                let dn = |c: usize| {
                    [
                        (4.0 * l[0] - 1.0) * dl[0][c],
                        (4.0 * l[1] - 1.0) * dl[1][c],
                        (4.0 * l[2] - 1.0) * dl[2][c],
                        4.0 * (l[0] * dl[1][c] + l[1] * dl[0][c]),
                        4.0 * (l[1] * dl[2][c] + l[2] * dl[1][c]),
                        4.0 * (l[2] * dl[0][c] + l[0] * dl[2][c]),
                    ]
                };
                let (dn_a, dn_b) = (dn(0), dn(1));
                let (mut ta, mut tb) = ([0.0; 3], [0.0; 3]);
                for (i, &ln) in nodes.iter().enumerate() {
                    for k in 0..3 {
                        ta[k] += dn_a[i] * coords[ln][k];
                        tb[k] += dn_b[i] * coords[ln][k];
                    }
                }
                let nrm = cross(ta, tb);
                for (i, &ln) in nodes.iter().enumerate() {
                    out.push((ln, nrm.map(|c| p * n[i] * c / 6.0)));
                }
            }
        }
    }
    Ok(out)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
