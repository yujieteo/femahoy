//! Linear elastic solid elements: `C3D8`, `C3D8I` and `C3D10`.
//!
//! Strain order is `[xx, yy, zz, xy, yz, xz]` with engineering shear strains.
//! Element matrices have 3 dofs per node and fixed sizes: 24 for the bricks,
//! 30 for the `C3D10`.
//!
//! The element API is [`evaluate`]. It takes the nodal displacements and the
//! state of the element, and it returns the internal force, the consistent
//! tangent and the stress of each integration point. A [`Material`] gives the
//! stress and the tangent for a strain. Linear elastic is the only material in M2.
//! The state ([`ElementState`]) holds the state of each integration point and
//! the internal dofs of the element (the 9 incompatible mode amplitudes of
//! `C3D8I`). A Newton loop must keep this state for each element between
//! iterations. The linear driver uses a scratch state and keeps nothing.

use crate::dense::{a_b, add_at_b, inv3, invert, Mat};
use crate::SolverError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Hex8 with B-bar (selective reduced integration of the volume strain).
    /// Plain `C3D8` is soft in bending on coarse meshes. This is expected.
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

    /// Number of integration points.
    pub fn point_count(self) -> usize {
        match self {
            Kind::C3D8 | Kind::C3D8I => 8,
            Kind::C3D10 => 4,
        }
    }
}

/// Largest number of dofs of one element.
pub const MAX_DOFS: usize = 30;
/// Largest number of integration points of one element.
pub const MAX_POINTS: usize = 8;

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

/// State of one integration point. History variables of later materials
/// (plastic strain, hardening) are added here in M6.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct IpState {
    pub strain: [f64; 6],
    pub stress: [f64; 6],
}

/// Material law at an integration point.
pub trait Material: Sync {
    /// Stress state and consistent tangent `d stress / d strain` for the total
    /// `strain`, from the converged state `old`. The call must not change `old`.
    fn update(&self, strain: &[f64; 6], old: &IpState) -> (IpState, [[f64; 6]; 6]);
}

/// Linear isotropic elasticity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elastic {
    pub d: [[f64; 6]; 6],
}

impl Elastic {
    pub fn new(youngs_modulus: f64, poissons_ratio: f64) -> Self {
        Elastic {
            d: elasticity(youngs_modulus, poissons_ratio),
        }
    }
}

impl Material for Elastic {
    fn update(&self, strain: &[f64; 6], _old: &IpState) -> (IpState, [[f64; 6]; 6]) {
        let mut stress = [0.0; 6];
        for (s, row) in stress.iter_mut().zip(&self.d) {
            *s = row.iter().zip(strain).map(|(d, e)| d * e).sum();
        }
        (
            IpState {
                strain: *strain,
                stress,
            },
            self.d,
        )
    }
}

/// State of one element between calls of [`evaluate`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState {
    pub ip: [IpState; MAX_POINTS],
    /// Internal dofs: the amplitudes of the 9 incompatible modes of `C3D8I`.
    /// Other kinds leave them at zero.
    pub internal: [f64; 9],
}

/// Result of [`evaluate`]. Only the first `n` entries of `fint` and the first
/// `n x n` block of `k` are valid.
#[derive(Clone)]
pub struct Evaluation {
    pub n: usize,
    pub fint: [f64; MAX_DOFS],
    /// Consistent tangent, with the internal dofs condensed out.
    pub k: Mat<MAX_DOFS, MAX_DOFS>,
    pub volume: f64,
    /// Volume-averaged stress.
    pub mean_stress: [f64; 6],
    /// Stress at each integration point. Only the first `Kind::point_count` entries are valid.
    pub ip_stress: [[f64; 6]; MAX_POINTS],
}

impl Default for Evaluation {
    fn default() -> Self {
        Evaluation {
            n: 0,
            fint: [0.0; MAX_DOFS],
            k: [[0.0; MAX_DOFS]; MAX_DOFS],
            volume: 0.0,
            mean_stress: [0.0; 6],
            ip_stress: [[0.0; 6]; MAX_POINTS],
        }
    }
}

/// One integration point: weight times Jacobian, strain-displacement matrix (6 x M)
/// and the incompatible-mode matrix (6 x 9) when the element has one.
#[derive(Clone, Copy)]
struct Point<const M: usize> {
    w: f64,
    b: Mat<6, M>,
    g: Option<Mat<6, 9>>,
}

impl<const M: usize> Point<M> {
    const ZERO: Self = Point {
        w: 0.0,
        b: [[0.0; M]; 6],
        g: None,
    };
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

fn jacobian(coords: &[[f64; 3]], dn: &[[f64; 3]]) -> Mat<3, 3> {
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
fn cartesian<const N: usize>(dn: &[[f64; 3]; N], jinv: &Mat<3, 3>) -> [[f64; 3]; N] {
    let mut out = [[0.0; 3]; N];
    for (c, d) in out.iter_mut().zip(dn) {
        for (i, ci) in c.iter_mut().enumerate() {
            *ci = (0..3).map(|j| d[j] * jinv[j][i]).sum();
        }
    }
    out
}

/// Strain-displacement matrix, 6 x M, with M = 3 times the number of nodes.
fn b_matrix<const M: usize>(dx: &[[f64; 3]]) -> Mat<6, M> {
    let mut b = [[0.0; M]; 6];
    for (a, d) in dx.iter().enumerate() {
        let c = 3 * a;
        b[0][c] = d[0];
        b[1][c + 1] = d[1];
        b[2][c + 2] = d[2];
        b[3][c] = d[1];
        b[3][c + 1] = d[0];
        b[4][c + 1] = d[2];
        b[4][c + 2] = d[1];
        b[5][c] = d[2];
        b[5][c + 2] = d[0];
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

fn check_nodes(kind: Kind, coords: &[[f64; 3]]) -> Result<(), SolverError> {
    if coords.len() == kind.node_count() {
        Ok(())
    } else {
        Err(SolverError::new(format!(
            "{kind:?} needs {} nodes, found {}",
            kind.node_count(),
            coords.len()
        )))
    }
}

fn tet10_points(coords: &[[f64; 3]]) -> Result<[Point<30>; MAX_POINTS], SolverError> {
    let (a, b) = (0.138_196_601_125_010_5, 0.585_410_196_624_968_5);
    let mut out = [Point::<30>::ZERO; MAX_POINTS];
    for (p, slot) in out.iter_mut().take(4).enumerate() {
        let mut l = [a; 4];
        l[p] = b;
        let dn = tet_dn(l);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, Kind::C3D10)?;
        *slot = Point {
            w: det / 24.0,
            b: b_matrix(&cartesian(&dn, &jinv)),
            g: None,
        };
    }
    Ok(out)
}

fn hex8_points(kind: Kind, coords: &[[f64; 3]]) -> Result<[Point<24>; MAX_POINTS], SolverError> {
    let g = 1.0 / 3.0_f64.sqrt();
    let mut out = [Point::<24>::ZERO; MAX_POINTS];
    let (jinv0, det0) = inv3(&jacobian(coords, &hex_dn([0.0; 3])));
    check_det(det0, kind)?;
    for (slot, s) in out.iter_mut().zip(HEX_XI) {
        let xi = [s[0] * g, s[1] * g, s[2] * g];
        let dn = hex_dn(xi);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, kind)?;
        let b = b_matrix(&cartesian(&dn, &jinv));
        let gm = (kind == Kind::C3D8I).then(|| {
            // Incompatible modes 1 - xi^2, 1 - eta^2, 1 - zeta^2: derivatives mapped
            // by the centre Jacobian and scaled by det0 / det. Column 3k + i is
            // mode k acting in direction i.
            let mut gm = [[0.0; 9]; 6];
            for k in 0..3 {
                let mut dnk = [[0.0; 3]; 1];
                dnk[0][k] = -2.0 * xi[k];
                let d = cartesian(&dnk, &jinv0)[0].map(|v| v * det0 / det);
                let bk: Mat<6, 3> = b_matrix(&[d]);
                for r in 0..6 {
                    gm[r][3 * k..3 * k + 3].copy_from_slice(&bk[r]);
                }
            }
            gm
        });
        *slot = Point { w: det, b, g: gm };
    }
    if kind == Kind::C3D8 {
        bbar(&mut out);
    }
    Ok(out)
}

/// Replace the volume strain part of each `B` with its element average.
fn bbar(points: &mut [Point<24>]) {
    let vol: f64 = points.iter().map(|p| p.w).sum();
    let mut avg = [0.0; 24];
    for p in points.iter() {
        for (c, a) in avg.iter_mut().enumerate() {
            *a += p.w * (p.b[0][c] + p.b[1][c] + p.b[2][c]);
        }
    }
    for a in &mut avg {
        *a /= vol;
    }
    for p in points.iter_mut() {
        for (c, a) in avg.iter().enumerate() {
            let local = p.b[0][c] + p.b[1][c] + p.b[2][c];
            let delta = (a - local) / 3.0;
            for r in 0..3 {
                p.b[r][c] += delta;
            }
        }
    }
}

fn mul_vec<const R: usize, const C: usize>(a: &Mat<R, C>, x: &[f64]) -> [f64; R] {
    let mut y = [0.0; R];
    for (yi, row) in y.iter_mut().zip(a) {
        *yi = row.iter().zip(x).map(|(a, x)| a * x).sum();
    }
    y
}

const MAX_MODE_ITERATIONS: usize = 6;

/// Evaluate one element. `u` holds the `3n` nodal displacements. The call
/// updates `state` (the points and the internal dofs) and writes `out`.
/// With `tangent` false the matrix `out.k` is left alone.
pub fn evaluate(
    kind: Kind,
    coords: &[[f64; 3]],
    material: &dyn Material,
    u: &[f64],
    state: &mut ElementState,
    out: &mut Evaluation,
    tangent: bool,
) -> Result<(), SolverError> {
    check_nodes(kind, coords)?;
    match kind {
        Kind::C3D10 => {
            let pts = tet10_points(coords)?;
            run::<30>(&pts[..4], material, u, state, out, tangent)
        }
        Kind::C3D8 | Kind::C3D8I => {
            let pts = hex8_points(kind, coords)?;
            run::<24>(&pts, material, u, state, out, tangent)
        }
    }
}

#[derive(Clone, Copy)]
struct Response {
    stress: [f64; 6],
    tangent: Mat<6, 6>,
}

fn run<const M: usize>(
    pts: &[Point<M>],
    material: &dyn Material,
    u: &[f64],
    state: &mut ElementState,
    out: &mut Evaluation,
    tangent: bool,
) -> Result<(), SolverError> {
    let has_modes = pts[0].g.is_some();
    let mut alpha = state.internal;
    let mut new_ip = state.ip;
    let mut resp = [Response {
        stress: [0.0; 6],
        tangent: [[0.0; 6]; 6],
    }; MAX_POINTS];
    // Strains of the compatible part. They do not change in the mode loop.
    let mut eps_u = [[0.0; 6]; MAX_POINTS];
    for (e, p) in eps_u.iter_mut().zip(pts) {
        *e = mul_vec(&p.b, u);
    }
    let mut kaa_inv = [[0.0; 9]; 9];
    let mut f_a;
    let mut iterations = 0;
    loop {
        f_a = [0.0; 9];
        let mut scale = 0.0;
        let mut kaa = [[0.0; 9]; 9];
        for (i, p) in pts.iter().enumerate() {
            let mut eps = eps_u[i];
            if let Some(g) = &p.g {
                let ga = mul_vec(g, &alpha);
                for (e, v) in eps.iter_mut().zip(ga) {
                    *e += v;
                }
            }
            let (s, c) = material.update(&eps, &state.ip[i]);
            new_ip[i] = s;
            resp[i] = Response {
                stress: s.stress,
                tangent: c,
            };
            if let Some(g) = &p.g {
                for a in 0..9 {
                    let t: f64 = (0..6).map(|r| g[r][a] * s.stress[r]).sum();
                    f_a[a] += p.w * t;
                    scale += (p.w * t).abs();
                }
                let cg = a_b(&c, g);
                add_at_b(&mut kaa, p.w, g, &cg);
            }
        }
        if !has_modes {
            break;
        }
        let max = f_a.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        kaa_inv =
            invert(&kaa).ok_or_else(|| SolverError::new("incompatible mode matrix is singular"))?;
        if max <= 1e-12 * scale || iterations == MAX_MODE_ITERATIONS {
            break;
        }
        // Newton step on the mode amplitudes: kaa * d_alpha = f_a.
        let da = mul_vec(&kaa_inv, &f_a);
        for (a, d) in alpha.iter_mut().zip(da) {
            *a -= d;
        }
        iterations += 1;
    }
    // Forces and tangent from the points.
    out.n = M;
    out.fint[..M].fill(0.0);
    let mut kuu = [[0.0; M]; M];
    let mut kua = [[0.0; 9]; M];
    let mut mean = [0.0; 6];
    let mut vol = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let r = &resp[i];
        for c in 0..M {
            let t: f64 = (0..6).map(|k| p.b[k][c] * r.stress[k]).sum();
            out.fint[c] += p.w * t;
        }
        if tangent || has_modes {
            let cb = a_b(&r.tangent, &p.b);
            if tangent {
                add_at_b(&mut kuu, p.w, &p.b, &cb);
            }
            if let Some(g) = &p.g {
                // kua = sum w Bᵀ C G.
                let cg = a_b(&r.tangent, g);
                add_at_b(&mut kua, p.w, &p.b, &cg);
            }
        }
        for (m, s) in mean.iter_mut().zip(&r.stress) {
            *m += p.w * s;
        }
        vol += p.w;
    }
    if has_modes {
        // Condense: f = f_u - Kua Kaa^-1 f_a, K = Kuu - Kua Kaa^-1 Kau.
        let q = a_b(&kua, &kaa_inv);
        for (c, row) in q.iter().enumerate() {
            out.fint[c] -= row.iter().zip(&f_a).map(|(a, b)| a * b).sum::<f64>();
        }
        if tangent {
            for i in 0..M {
                for j in 0..M {
                    kuu[i][j] -= (0..9).map(|a| q[i][a] * kua[j][a]).sum::<f64>();
                }
            }
        }
    }
    if tangent {
        for (dst, src) in out.k.iter_mut().zip(&kuu) {
            dst[..M].copy_from_slice(src);
        }
    }
    out.volume = vol;
    out.mean_stress = mean.map(|v| v / vol);
    for (dst, ip) in out.ip_stress.iter_mut().zip(&new_ip) {
        *dst = ip.stress;
    }
    state.ip = new_ip;
    state.internal = alpha;
    Ok(())
}

/// Element stiffness, `3n x 3n` row-major.
pub fn stiffness(
    kind: Kind,
    coords: &[[f64; 3]],
    d: &[[f64; 6]; 6],
) -> Result<Vec<f64>, SolverError> {
    let mut out = Evaluation::default();
    let u = [0.0; MAX_DOFS];
    let n = 3 * kind.node_count();
    evaluate(
        kind,
        coords,
        &Elastic { d: *d },
        &u[..n],
        &mut ElementState::default(),
        &mut out,
        true,
    )?;
    Ok(out.k[..n].iter().flat_map(|r| r[..n].to_vec()).collect())
}

/// Element volume.
pub fn volume(kind: Kind, coords: &[[f64; 3]]) -> Result<f64, SolverError> {
    check_nodes(kind, coords)?;
    Ok(match kind {
        Kind::C3D10 => tet10_points(coords)?.iter().map(|p| p.w).sum(),
        _ => hex8_points(kind, coords)?.iter().map(|p| p.w).sum(),
    })
}

/// Volume-averaged stress of the element for the nodal displacements `u`.
pub fn mean_stress(
    kind: Kind,
    coords: &[[f64; 3]],
    d: &[[f64; 6]; 6],
    u: &[f64],
) -> Result<[f64; 6], SolverError> {
    let mut out = Evaluation::default();
    evaluate(
        kind,
        coords,
        &Elastic { d: *d },
        u,
        &mut ElementState::default(),
        &mut out,
        false,
    )?;
    Ok(out.mean_stress)
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
