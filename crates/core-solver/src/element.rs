//! Linear elastic solid elements: `C3D8`, `C3D8I` and `C3D10`.
//!
//! Strain order is `[xx, yy, zz, xy, yz, xz]` with engineering shear strains.
//! Element matrices have 3 dofs per node and fixed sizes: 24 for the bricks,
//! 30 for the `C3D10`.
//!
//! The element API is [`evaluate`]. It is a pure function on fixed-size arrays: it
//! takes the node positions ([`Placement`], start position plus displacement), a
//! [`Material`] and the committed state of the element ([`ElementState`]). It writes
//! the internal force, the consistent tangent, the stress of each integration point
//! and the new (trial) state into an [`Evaluation`]. It does not allocate. It does
//! not change its inputs. The kernel is generic over the material, so a backend
//! that has one material type (for example on a GPU) needs no trait object.
//!
//! The element gives the material the deformation gradient `F` of each integration
//! point. The material returns the stress, the tangent `d stress / d strain` and
//! its new state. The state of an integration point belongs to the material
//! (`Material::state_len` numbers; elastic has none). The element-internal state
//! (the 9 incompatible mode amplitudes of `C3D8I`) is separate. The caller keeps a
//! committed and a trial copy (see `state`). Elements know no material constants.

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

/// Largest number of state numbers that a material keeps at one integration point.
pub const MAX_MAT_STATE: usize = 8;

/// Positions of the nodes of one element: the start position and the displacement
/// (`3n` numbers). The current position of node `a` is start + displacement.
#[derive(Debug, Clone, Copy)]
pub struct Placement<'a> {
    pub start: &'a [[f64; 3]],
    pub disp: &'a [f64],
}

impl Placement<'_> {
    pub fn current(&self, a: usize) -> [f64; 3] {
        let u = &self.disp[3 * a..3 * a + 3];
        let x = self.start[a];
        [x[0] + u[0], x[1] + u[1], x[2] + u[2]]
    }
}

/// The deformation gradient `F = I + h` of an integration point. It keeps the
/// displacement gradient `h`, because `I + h` rounds away a normal strain smaller
/// than the machine epsilon. Compute a strain from `h`, not from `f() - I`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gradient {
    /// The displacement gradient, `h[i][j] = d u_i / d X_j`.
    pub h: [[f64; 3]; 3],
}

impl Gradient {
    /// The deformation gradient `F = I + h`.
    pub fn f(&self) -> [[f64; 3]; 3] {
        let mut f = self.h;
        for (i, fi) in f.iter_mut().enumerate() {
            fi[i] += 1.0;
        }
        f
    }

    /// Small strain `[xx, yy, zz, xy, yz, xz]` (engineering shear).
    pub fn small_strain(&self) -> [f64; 6] {
        let h = &self.h;
        [
            h[0][0],
            h[1][1],
            h[2][2],
            h[0][1] + h[1][0],
            h[1][2] + h[2][1],
            h[0][2] + h[2][0],
        ]
    }
}

/// Material law at an integration point. It is separate from the element and owns the
/// state of the point, so plasticity (M5, M6) and large displacement (M7) use this interface.
/// Decision D-10, see DESIGN.md.
pub trait Material: Sync {
    /// Numbers of state at each integration point. Elastic has none.
    fn state_len(&self) -> usize {
        0
    }

    /// False when the stress depends on the history or is not linear in the strain.
    /// A run with only linear materials takes the linear fallback.
    fn is_linear(&self) -> bool {
        true
    }

    /// Stress `[xx, yy, zz, xy, yz, xz]` and the consistent tangent `d stress / d strain`
    /// for the deformation gradient `g`. `old` is the committed state (`state_len`
    /// numbers) and the call writes the trial state in `new`. An error means that the
    /// trial is not acceptable, and the step cuts the increment back.
    fn update(
        &self,
        g: &Gradient,
        old: &[f64],
        new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError>;
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
    fn update(
        &self,
        g: &Gradient,
        _old: &[f64],
        _new: &mut [f64],
    ) -> Result<([f64; 6], [[f64; 6]; 6]), SolverError> {
        let strain = g.small_strain();
        let mut stress = [0.0; 6];
        for (s, row) in stress.iter_mut().zip(&self.d) {
            *s = row.iter().zip(&strain).map(|(d, e)| d * e).sum();
        }
        Ok((stress, self.d))
    }
}

/// Committed state of one element: the material state at each integration point
/// (point `i`, number `c` at `ip[i][c]`) and the internal dofs of the element.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElementState {
    pub ip: [[f64; MAX_MAT_STATE]; MAX_POINTS],
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
    /// Trial state: material state of each point and the internal dofs.
    pub ip_state: [[f64; MAX_MAT_STATE]; MAX_POINTS],
    pub internal: [f64; 9],
    /// Sum of the absolute node forces of the points, a scale for the Newton check.
    pub fabs: [f64; MAX_DOFS],
}

impl Evaluation {
    /// The trial state as one flat slice: point `i`, number `c` of an element is at
    /// `i * MAX_MAT_STATE + c`. A contribution group writes its state here.
    pub fn state(&self) -> &[f64] {
        self.ip_state.as_flattened()
    }

    pub fn state_mut(&mut self) -> &mut [f64] {
        self.ip_state.as_flattened_mut()
    }
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
            ip_state: [[0.0; MAX_MAT_STATE]; MAX_POINTS],
            internal: [0.0; 9],
            fabs: [0.0; MAX_DOFS],
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
    /// Cartesian derivatives of the shape functions (the first `M / 3` rows are used).
    dx: [[f64; 3]; 10],
    /// Cartesian derivatives of the three incompatible modes.
    dm: [[f64; 3]; 3],
}

impl<const M: usize> Point<M> {
    const ZERO: Self = Point {
        w: 0.0,
        b: [[0.0; M]; 6],
        g: None,
        dx: [[0.0; 3]; 10],
        dm: [[0.0; 3]; 3],
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

fn check_det(det: f64, jinv: &Mat<3, 3>, kind: Kind) -> Result<(), SolverError> {
    if det.is_nan() || det <= 0.0 {
        Err(SolverError::new(format!(
            "{kind:?} element has a non-positive Jacobian ({det:e})"
        )))
    } else if !det.is_finite() || jinv.iter().flatten().any(|v| !v.is_finite()) {
        Err(SolverError::new(format!(
            "{kind:?} element has a Jacobian or an inverse Jacobian that is not finite ({det:e})"
        )))
    } else {
        Ok(())
    }
}

fn check_nodes(kind: Kind, coords: &[[f64; 3]]) -> Result<(), SolverError> {
    if coords.len() != kind.node_count() {
        Err(SolverError::new(format!(
            "{kind:?} needs {} nodes, found {}",
            kind.node_count(),
            coords.len()
        )))
    } else if coords.iter().flatten().any(|v| !v.is_finite()) {
        Err(SolverError::new(format!(
            "{kind:?} element has a node coordinate that is not finite"
        )))
    } else {
        Ok(())
    }
}

/// The node count, and finite start positions and displacements.
fn check_place(kind: Kind, place: &Placement) -> Result<(), SolverError> {
    check_nodes(kind, place.start)?;
    let n = 3 * place.start.len();
    if place.disp.len() < n {
        return Err(SolverError::new("too few displacements for the nodes"));
    }
    if place.disp[..n].iter().any(|v| !v.is_finite()) {
        return Err(SolverError::new(format!(
            "{kind:?} element has a displacement that is not finite"
        )));
    }
    Ok(())
}

fn tet10_points(coords: &[[f64; 3]]) -> Result<[Point<30>; MAX_POINTS], SolverError> {
    let (a, b) = (0.138_196_601_125_010_5, 0.585_410_196_624_968_5);
    let mut out = [Point::<30>::ZERO; MAX_POINTS];
    for (p, slot) in out.iter_mut().take(4).enumerate() {
        let mut l = [a; 4];
        l[p] = b;
        let dn = tet_dn(l);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, &jinv, Kind::C3D10)?;
        let dx = cartesian(&dn, &jinv);
        *slot = Point {
            w: det / 24.0,
            b: b_matrix(&dx),
            g: None,
            dx,
            dm: [[0.0; 3]; 3],
        };
    }
    Ok(out)
}

fn hex8_points(kind: Kind, coords: &[[f64; 3]]) -> Result<[Point<24>; MAX_POINTS], SolverError> {
    let g = 1.0 / 3.0_f64.sqrt();
    let mut out = [Point::<24>::ZERO; MAX_POINTS];
    let (jinv0, det0) = inv3(&jacobian(coords, &hex_dn([0.0; 3])));
    check_det(det0, &jinv0, kind)?;
    for (slot, s) in out.iter_mut().zip(HEX_XI) {
        let xi = [s[0] * g, s[1] * g, s[2] * g];
        let dn = hex_dn(xi);
        let (jinv, det) = inv3(&jacobian(coords, &dn));
        check_det(det, &jinv, kind)?;
        let dx8 = cartesian(&dn, &jinv);
        let b = b_matrix(&dx8);
        let mut dxs = [[0.0; 3]; 10];
        dxs[..8].copy_from_slice(&dx8);
        let mut dm = [[0.0; 3]; 3];
        let gm = (kind == Kind::C3D8I).then(|| {
            // Incompatible modes 1 - xi^2, 1 - eta^2, 1 - zeta^2: derivatives mapped
            // by the centre Jacobian and scaled by det0 / det. Column 3k + i is
            // mode k acting in direction i.
            let mut gm = [[0.0; 9]; 6];
            for k in 0..3 {
                let mut dnk = [[0.0; 3]; 1];
                dnk[0][k] = -2.0 * xi[k];
                let d = cartesian(&dnk, &jinv0)[0].map(|v| v * det0 / det);
                dm[k] = d;
                let bk: Mat<6, 3> = b_matrix(&[d]);
                for r in 0..6 {
                    gm[r][3 * k..3 * k + 3].copy_from_slice(&bk[r]);
                }
            }
            gm
        });
        *slot = Point {
            w: det,
            b,
            g: gm,
            dx: dxs,
            dm,
        };
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

/// Evaluate one element. `state` is the committed state. The call writes the
/// internal force, the stress, the trial state and, with `tangent`, the matrix into
/// `out`. With `tangent` false the matrix `out.k` is left alone.
///
/// The geometry of the strain-displacement matrices is the start geometry (small
/// displacements). The current positions enter through the deformation gradient
/// and, in M7, through the geometry of the kernel.
pub fn evaluate<M: Material + ?Sized>(
    kind: Kind,
    place: &Placement,
    material: &M,
    state: &ElementState,
    out: &mut Evaluation,
    tangent: bool,
) -> Result<(), SolverError> {
    let coords = place.start;
    check_place(kind, place)?;
    match kind {
        Kind::C3D10 => {
            let pts = tet10_points(coords)?;
            run::<30, M>(&pts[..4], material, place.disp, state, out, tangent)
        }
        Kind::C3D8 | Kind::C3D8I => {
            let pts = hex8_points(kind, coords)?;
            run::<24, M>(&pts, material, place.disp, state, out, tangent)
        }
    }
}

#[derive(Clone, Copy)]
struct Response {
    stress: [f64; 6],
    tangent: Mat<6, 6>,
}

/// Deformation gradient of a point. The compatible part comes from the node
/// displacements, the incompatible part from the mode amplitudes. `eps` is the strain
/// that the element feels (B-bar changes its volume part), and the gradient is
/// corrected to carry the same volume strain.
fn gradient<const M: usize>(p: &Point<M>, u: &[f64], alpha: &[f64; 9], eps: &[f64; 6]) -> Gradient {
    let mut h = [[0.0; 3]; 3];
    for a in 0..M / 3 {
        for (i, hi) in h.iter_mut().enumerate() {
            for (j, hij) in hi.iter_mut().enumerate() {
                *hij += u[3 * a + i] * p.dx[a][j];
            }
        }
    }
    if p.g.is_some() {
        for k in 0..3 {
            for (i, hi) in h.iter_mut().enumerate() {
                for (j, hij) in hi.iter_mut().enumerate() {
                    *hij += alpha[3 * k + i] * p.dm[k][j];
                }
            }
        }
    }
    let shift = (eps[0] + eps[1] + eps[2] - (h[0][0] + h[1][1] + h[2][2])) / 3.0;
    for (i, hi) in h.iter_mut().enumerate() {
        hi[i] += shift;
    }
    Gradient { h }
}

fn run<const M: usize, Mt: Material + ?Sized>(
    pts: &[Point<M>],
    material: &Mt,
    u: &[f64],
    state: &ElementState,
    out: &mut Evaluation,
    tangent: bool,
) -> Result<(), SolverError> {
    let has_modes = pts[0].g.is_some();
    let sl = material.state_len();
    if sl > MAX_MAT_STATE {
        return Err(SolverError::new(format!(
            "the material keeps {sl} state numbers, the limit is {MAX_MAT_STATE}"
        )));
    }
    let mut alpha = state.internal;
    let mut new_ip = [[0.0; MAX_MAT_STATE]; MAX_POINTS];
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
            let ga = p.g.as_ref().map_or([0.0; 6], |g| mul_vec(g, &alpha));
            for (e, v) in eps.iter_mut().zip(ga) {
                *e += v;
            }
            let g = gradient(p, u, &alpha, &eps);
            let (stress, c) = material.update(&g, &state.ip[i][..sl], &mut new_ip[i][..sl])?;
            if stress
                .iter()
                .chain(c.iter().flatten())
                .any(|v| !v.is_finite())
            {
                return Err(SolverError::new(
                    "the material gave a stress or a tangent that is not finite",
                ));
            }
            resp[i] = Response { stress, tangent: c };
            if let Some(g) = &p.g {
                // The size of the stress before the compatible strain and the mode strain
                // cancel. The rounding of the mode force is relative to this size.
                let mut size = stress.map(f64::abs);
                for (s, row) in size.iter_mut().zip(&c) {
                    for (k, ck) in row.iter().enumerate() {
                        *s += ck.abs() * (eps_u[i][k].abs() + ga[k].abs());
                    }
                }
                for a in 0..9 {
                    let t: f64 = (0..6).map(|r| g[r][a] * stress[r]).sum();
                    f_a[a] += p.w * t;
                    scale += (0..6).map(|r| (p.w * g[r][a] * size[r]).abs()).sum::<f64>();
                }
                let cg = a_b(&c, g);
                add_at_b(&mut kaa, p.w, g, &cg);
            }
        }
        if !has_modes {
            break;
        }
        if !(scale.is_finite() && f_a.iter().all(|v| v.is_finite())) {
            return Err(SolverError::new(
                "the stress of the incompatible modes is not finite",
            ));
        }
        let max = f_a.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        kaa_inv =
            invert(&kaa).ok_or_else(|| SolverError::new("incompatible mode matrix is singular"))?;
        if max <= 1e-12 * scale {
            break;
        }
        // The step cuts the increment back on this error.
        if iterations == MAX_MODE_ITERATIONS {
            return Err(SolverError::new(format!(
                "incompatible modes did not converge in {MAX_MODE_ITERATIONS} iterations \
                 (mode force {max:e}, scale {scale:e})"
            )));
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
    out.fabs[..M].fill(0.0);
    let mut kuu = [[0.0; M]; M];
    let mut kua = [[0.0; 9]; M];
    let mut mean = [0.0; 6];
    let mut vol = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let r = &resp[i];
        for c in 0..M {
            let t: f64 = (0..6).map(|k| p.b[k][c] * r.stress[k]).sum();
            out.fint[c] += p.w * t;
            out.fabs[c] += (p.w * t).abs();
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
    // Finite inputs can give a force or a stiffness that overflows.
    let k_bad = tangent && kuu.iter().flatten().any(|v| !v.is_finite());
    if k_bad || out.fint[..M].iter().any(|v| !v.is_finite()) || !vol.is_finite() {
        return Err(SolverError::new(
            "the element force or stiffness is not finite",
        ));
    }
    if tangent {
        for (dst, src) in out.k.iter_mut().zip(&kuu) {
            dst[..M].copy_from_slice(src);
        }
    }
    out.volume = vol;
    out.mean_stress = mean.map(|v| v / vol);
    for (dst, r) in out.ip_stress.iter_mut().zip(&resp) {
        *dst = r.stress;
    }
    out.ip_state = new_ip;
    out.internal = alpha;
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
        &Placement {
            start: coords,
            disp: &u[..n],
        },
        &Elastic { d: *d },
        &ElementState::default(),
        &mut out,
        true,
    )?;
    Ok(out.k[..n].iter().flat_map(|r| r[..n].to_vec()).collect())
}

/// Element volume.
pub fn volume(kind: Kind, coords: &[[f64; 3]]) -> Result<f64, SolverError> {
    check_nodes(kind, coords)?;
    Ok(match kind {
        Kind::C3D10 => {
            // The points of the stiffness check the element. The determinant of a curved
            // C3D10 has degree 3, and the stiffness rule is exact only to degree 2. This
            // 5-point rule is exact to degree 3: the centre with weight -4/5 and 4 points
            // with weight 9/20, each times the reference volume 1/6.
            tet10_points(coords)?;
            let (a, b) = (0.5, 1.0 / 6.0);
            let mut v = 0.0;
            for (l, w) in [
                ([0.25; 4], -0.8),
                ([a, b, b, b], 0.45),
                ([b, a, b, b], 0.45),
                ([b, b, a, b], 0.45),
                ([b, b, b, a], 0.45),
            ] {
                let (jinv, det) = inv3(&jacobian(coords, &tet_dn(l)));
                check_det(det, &jinv, Kind::C3D10)?;
                v += w * det / 6.0;
            }
            v
        }
        // The determinant of a hex8 has degree 2 in each direction. The 2-point rule is
        // exact to degree 3.
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
        &Placement {
            start: coords,
            disp: u,
        },
        &Elastic { d: *d },
        &ElementState::default(),
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
/// Positive pressure pushes into the body. The face is taken at the current positions
/// (start + displacement). Returns `(local node, force)` pairs.
pub fn pressure_load(
    kind: Kind,
    place: &Placement,
    face: u8,
    p: f64,
) -> Result<Vec<(usize, [f64; 3])>, SolverError> {
    check_place(kind, place)?;
    let bad = || SolverError::new(format!("{kind:?} has no face {face}"));
    let f = usize::from(face).checked_sub(1).ok_or_else(bad)?;
    let mut now = [[0.0; 3]; 10];
    for (a, x) in now.iter_mut().enumerate().take(place.start.len()) {
        *x = place.current(a);
    }
    let coords = &now[..place.start.len()];
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
            // A quadratic shape function times the quadratic area vector of a curved face
            // has degree 4. The symmetric 6-point rule on the triangle is exact to degree 4.
            // Each weight includes the reference area 1/2.
            let (a, wa) = (0.445_948_490_915_964_9, 0.111_690_794_839_005_7);
            let (b, wb) = (0.091_576_213_509_770_74, 0.054_975_871_827_660_93);
            for (l1, l2, w) in [
                (a, a, wa),
                (1.0 - 2.0 * a, a, wa),
                (a, 1.0 - 2.0 * a, wa),
                (b, b, wb),
                (1.0 - 2.0 * b, b, wb),
                (b, 1.0 - 2.0 * b, wb),
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
                    out.push((ln, nrm.map(|c| p * n[i] * c * w)));
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
