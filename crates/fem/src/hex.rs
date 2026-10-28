//! The 8-node hex element of linear elasticity with incompatible modes, on a box cell.
//!
//! The displacement is the trilinear field of the 8 corners plus 9 incompatible modes: the
//! bubbles 1 - xi^2, 1 - eta^2 and 1 - zeta^2 of each component. The modes are internal to the
//! cell, so static condensation removes them before the assembly. With them, a box cell holds a
//! pure bending field exactly, so a thin layer with 2 cells through its thickness does not lock
//! in bending, as the plain 8-node hex does. A box cell has a constant Jacobian, so the strain of
//! the modes has a mean of 0 in the cell, and the element passes the patch test (Taylor,
//! Beresford and Wilson). The material is orthotropic with its axes on x, y and z, which also
//! holds the isotropic material.
//!
//! The thermal strain is alpha (T - T_ref) on the normal strains, with the temperature
//! interpolated from the 8 corners. Its load on the modes is condensed with them, so a linear
//! temperature through a layer gives the exact bending of the cell too.
//!
//! The stress is D (B u + G a - alpha N T): B of the corners, G of the modes, `a` the amplitudes
//! of the modes from the condensation.
//!
//! The 2 x 2 x 2 Gauss rule integrates each matrix of a box cell exactly.
//!
//! The consistent mass is that of the trilinear field of the 8 corners: the incompatible modes
//! have no mass, as in Wilson et al. They are internal strain modes, not a displacement of the
//! cell that a frequency solve must carry.
//!
//! Sources: E. L. Wilson, R. L. Taylor, W. P. Doherty and J. Ghaboussi, Incompatible
//! displacement models, in Numerical and Computer Methods in Structural Mechanics, Academic Press
//! 1973, 43-57; R. L. Taylor, P. J. Beresford and E. L. Wilson, A non-conforming element for
//! stress analysis, Int. J. Numer. Meth. Engng 10 (1976) 1211-1219; T. J. R. Hughes, The Finite
//! Element Method, Prentice-Hall 1987, sections 2.7 (the elasticity matrices) and 4.7
//! (incompatible modes); R. M. Jones, Mechanics of Composite Materials, 2nd ed., Taylor and
//! Francis 1999, section 2.5 (the engineering constants of an orthotropic material).

/// The local bits of the 8 corners, in the order of a VTK hexahedron.
pub const BITS: [[usize; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];

/// An orthotropic material with its axes on x, y and z.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orthotropic {
    /// Young's modulus along x, y and z.
    pub e: [f64; 3],
    /// The Poisson ratios nu_xy, nu_xz and nu_yz: nu_ij is -(strain j) / (strain i) under a
    /// stress along i.
    pub nu: [f64; 3],
    /// The shear moduli G_xy, G_xz and G_yz.
    pub g: [f64; 3],
}

impl Orthotropic {
    pub fn isotropic(e: f64, nu: f64) -> Orthotropic {
        Orthotropic {
            e: [e; 3],
            nu: [nu; 3],
            g: [e / (2.0 * (1.0 + nu)); 3],
        }
    }

    /// The stiffness of the normal stresses (3 x 3) and the shear moduli G_yz, G_xz and G_xy, in
    /// the order of the strain vector. An error tells that the constants give no stable
    /// material.
    pub fn stiffness(&self) -> Result<Stiffness, String> {
        let [ex, ey, ez] = self.e;
        let [nxy, nxz, nyz] = self.nu;
        if !self
            .e
            .iter()
            .chain(&self.g)
            .all(|v| v.is_finite() && *v > 0.0)
            || !self.nu.iter().all(|v| v.is_finite())
        {
            return Err("the moduli must be above 0 and the Poisson ratios finite".into());
        }
        // The compliance of the normal stresses: S_ij = -nu_ij / E_i, symmetric.
        let s = [
            [1.0 / ex, -nxy / ex, -nxz / ex],
            [-nxy / ex, 1.0 / ey, -nyz / ey],
            [-nxz / ex, -nyz / ey, 1.0 / ez],
        ];
        let det = s[0][0] * (s[1][1] * s[2][2] - s[1][2] * s[2][1])
            - s[0][1] * (s[1][0] * s[2][2] - s[1][2] * s[2][0])
            + s[0][2] * (s[1][0] * s[2][1] - s[1][1] * s[2][0]);
        let minor = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        // The compliance is positive definite when its leading minors are above 0.
        if !(minor > 0.0 && det > 1e-12 * s[0][0] * s[1][1] * s[2][2]) {
            return Err(
                "the Poisson ratios give no stable material (for an isotropic material, nu must \
                 be between -1 and 0.5)"
                    .into(),
            );
        }
        let mut c = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                let (i1, i2) = ((i + 1) % 3, (i + 2) % 3);
                let (j1, j2) = ((j + 1) % 3, (j + 2) % 3);
                // The cofactor of s[j][i] over the determinant: the inverse of a symmetric
                // matrix.
                c[i][j] = (s[j1][i1] * s[j2][i2] - s[j1][i2] * s[j2][i1]) / det;
            }
        }
        let [gxy, gxz, gyz] = self.g;
        Ok(Stiffness {
            normal: c,
            shear: [gyz, gxz, gxy],
        })
    }
}

/// The stiffness of an orthotropic material. The strain and the stress vectors are xx, yy, zz,
/// yz, xz, xy, with the engineering shear strains.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stiffness {
    pub normal: [[f64; 3]; 3],
    /// G_yz, G_xz, G_xy.
    pub shear: [f64; 3],
}

impl Stiffness {
    /// sigma = D epsilon.
    pub fn stress(&self, e: &[f64; 6]) -> [f64; 6] {
        let n = &self.normal;
        [
            n[0][0] * e[0] + n[0][1] * e[1] + n[0][2] * e[2],
            n[1][0] * e[0] + n[1][1] * e[1] + n[1][2] * e[2],
            n[2][0] * e[0] + n[2][1] * e[1] + n[2][2] * e[2],
            self.shear[0] * e[3],
            self.shear[1] * e[4],
            self.shear[2] * e[5],
        ]
    }
}

/// The 2 axes of a shear strain: yz, xz, xy.
const SHEAR: [[usize; 2]; 3] = [[1, 2], [0, 2], [0, 1]];

/// The gradients of the 8 corner functions and the 3 mode functions at the local point `xi`
/// (each coordinate -1 to 1) of a box with the side lengths `size`.
fn gradients(size: [f64; 3], xi: [f64; 3]) -> [[f64; 3]; 11] {
    let mut g = [[0.0; 3]; 11];
    for (n, b) in BITS.iter().enumerate() {
        let s: [f64; 3] = std::array::from_fn(|d| if b[d] == 1 { 1.0 } else { -1.0 });
        let f: [f64; 3] = std::array::from_fn(|d| 0.5 * (1.0 + s[d] * xi[d]));
        for d in 0..3 {
            let (d1, d2) = ((d + 1) % 3, (d + 2) % 3);
            g[n][d] = s[d] / size[d] * f[d1] * f[d2];
        }
    }
    // d(1 - xi^2)/dx = -2 xi (2 / a).
    for d in 0..3 {
        g[8 + d][d] = -4.0 * xi[d] / size[d];
    }
    g
}

/// The values of the 8 corner functions at the local point `xi`.
pub fn shape(xi: [f64; 3]) -> [f64; 8] {
    std::array::from_fn(|n| {
        let b = BITS[n];
        (0..3)
            .map(|d| {
                if b[d] == 1 {
                    0.5 * (1.0 + xi[d])
                } else {
                    0.5 * (1.0 - xi[d])
                }
            })
            .product()
    })
}

/// The strain at a point from the gradients `g` of the 11 functions and their displacements.
fn strain(g: &[[f64; 3]; 11], u: &[[f64; 3]; 11]) -> [f64; 6] {
    let mut e = [0.0; 6];
    for (gi, ui) in g.iter().zip(u) {
        for d in 0..3 {
            e[d] += gi[d] * ui[d];
        }
        for (s, [a, b]) in SHEAR.iter().enumerate() {
            e[3 + s] += gi[*a] * ui[*b] + gi[*b] * ui[*a];
        }
    }
    e
}

/// The 2-point Gauss points of the cell, each coordinate -1 or 1 over sqrt 3.
fn gauss() -> [[f64; 3]; 8] {
    let p = 1.0 / 3f64.sqrt();
    BITS.map(|b| b.map(|v| if v == 1 { p } else { -p }))
}

/// The condensed element of a box cell.
#[derive(Debug, Clone)]
pub struct Hex {
    size: [f64; 3],
    d: Stiffness,
    /// The thermal stress of a unit temperature change: D alpha on the normal stresses.
    thermal: [f64; 3],
    /// The stiffness of the 24 corner unknowns (3 node + component), row-major.
    pub k: [f64; 576],
    /// The load of a unit temperature change at each corner (24 x 8, row-major).
    pub load: [f64; 192],
    /// The mode amplitudes: a = mode_t T - mode_u u (9 x 8 and 9 x 24, row-major).
    mode_u: [f64; 216],
    mode_t: [f64; 72],
}

impl Hex {
    /// The element of a box with the side lengths `size`, the stiffness `d` and the expansion
    /// coefficients `alpha` along x, y and z.
    pub fn new(size: [f64; 3], d: &Stiffness, alpha: [f64; 3]) -> Hex {
        let thermal: [f64; 3] =
            std::array::from_fn(|i| (0..3).map(|j| d.normal[i][j] * alpha[j]).sum());
        // The full matrices of the 33 unknowns: 11 functions times 3 components.
        let mut k = [[0.0; 33]; 33];
        let mut l = [[0.0; 8]; 33];
        let jac = size[0] * size[1] * size[2] / 8.0;
        for xi in gauss() {
            let g = gradients(size, xi);
            let n = shape(xi);
            for i in 0..11 {
                let gi = g[i];
                for j in 0..11 {
                    let gj = g[j];
                    // The 3 x 3 block of 2 functions: the normal terms of D, and the shear
                    // terms G_ab (g_i[b] g_j[b] on the diagonal, g_i[b] g_j[a] off it).
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
                                let s = 3 - a - b;
                                v += gi[b] * gj[a] * d.shear[s];
                            }
                            k[3 * i + a][3 * j + b] += jac * v;
                        }
                    }
                }
                for a in 0..3 {
                    for (c, nc) in n.iter().enumerate() {
                        l[3 * i + a][c] += jac * gi[a] * thermal[a] * nc;
                    }
                }
            }
        }
        // Condense the 9 modes: X = K_mm^-1 [K_mu | L_m].
        let mut kmm = [[0.0; 9]; 9];
        for i in 0..9 {
            for j in 0..9 {
                kmm[i][j] = k[24 + i][24 + j];
            }
        }
        let chol = cholesky9(&kmm);
        let mut mode_u = [0.0; 216];
        let mut mode_t = [0.0; 72];
        for c in 0..24 {
            let col = solve9(&chol, std::array::from_fn(|i| k[24 + i][c]));
            for i in 0..9 {
                mode_u[i * 24 + c] = col[i];
            }
        }
        for c in 0..8 {
            let col = solve9(&chol, std::array::from_fn(|i| l[24 + i][c]));
            for i in 0..9 {
                mode_t[i * 8 + c] = col[i];
            }
        }
        let mut kc = [0.0; 576];
        let mut lc = [0.0; 192];
        for r in 0..24 {
            for c in 0..24 {
                let mut v = k[r][c];
                for i in 0..9 {
                    v -= k[r][24 + i] * mode_u[i * 24 + c];
                }
                kc[r * 24 + c] = v;
            }
            for c in 0..8 {
                let mut v = l[r][c];
                for i in 0..9 {
                    v -= k[r][24 + i] * mode_t[i * 8 + c];
                }
                lc[r * 8 + c] = v;
            }
        }
        Hex {
            size,
            d: *d,
            thermal,
            k: kc,
            load: lc,
            mode_u,
            mode_t,
        }
    }

    /// The total strain and the stress at the local point `xi` (each coordinate -1 to 1), from
    /// the corner displacements `u` and the corner temperature changes `t`.
    pub fn strain_stress(&self, xi: [f64; 3], u: &[f64; 24], t: &[f64; 8]) -> ([f64; 6], [f64; 6]) {
        let mut all = [[0.0; 3]; 11];
        for n in 0..8 {
            all[n] = [u[3 * n], u[3 * n + 1], u[3 * n + 2]];
        }
        for i in 0..9 {
            let mut a = 0.0;
            for c in 0..8 {
                a += self.mode_t[i * 8 + c] * t[c];
            }
            for c in 0..24 {
                a -= self.mode_u[i * 24 + c] * u[c];
            }
            all[8 + i / 3][i % 3] = a;
        }
        let e = strain(&gradients(self.size, xi), &all);
        let dt: f64 = shape(xi).iter().zip(t).map(|(n, t)| n * t).sum();
        let mut s = self.d.stress(&e);
        for i in 0..3 {
            s[i] -= self.thermal[i] * dt;
        }
        (e, s)
    }
}

/// The consistent mass of the 24 corner unknowns of a box with the side lengths `size` and the
/// density `density` (row-major): the integral of density N_i N_j on each component. The
/// trilinear functions are products of 1 function of each axis, so each entry is the product of
/// the 1D masses a/3 (the same end) and a/6 (2 ends).
pub fn mass(size: [f64; 3], density: f64) -> [f64; 576] {
    let mut m = [0.0; 576];
    for i in 0..8 {
        for j in 0..8 {
            let v: f64 = (0..3)
                .map(|d| {
                    if BITS[i][d] == BITS[j][d] {
                        size[d] / 3.0
                    } else {
                        size[d] / 6.0
                    }
                })
                .product();
            for a in 0..3 {
                m[(3 * i + a) * 24 + 3 * j + a] = density * v;
            }
        }
    }
    m
}

/// The Cholesky factor of a 9 x 9 symmetric positive definite matrix (lower triangle).
fn cholesky9(a: &[[f64; 9]; 9]) -> [[f64; 9]; 9] {
    let mut l = [[0.0; 9]; 9];
    for j in 0..9 {
        let mut d = a[j][j];
        for k in 0..j {
            d -= l[j][k] * l[j][k];
        }
        let d = d.sqrt();
        l[j][j] = d;
        for i in j + 1..9 {
            let mut v = a[i][j];
            for k in 0..j {
                v -= l[i][k] * l[j][k];
            }
            l[i][j] = v / d;
        }
    }
    l
}

fn solve9(l: &[[f64; 9]; 9], mut b: [f64; 9]) -> [f64; 9] {
    for i in 0..9 {
        for k in 0..i {
            b[i] -= l[i][k] * b[k];
        }
        b[i] /= l[i][i];
    }
    for i in (0..9).rev() {
        for k in i + 1..9 {
            b[i] -= l[k][i] * b[k];
        }
        b[i] /= l[i][i];
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [f64; 3] = [2.0, 0.5, 0.1];

    /// The mass of the corners equals the 2-point Gauss integral of rho N_i N_j, which is exact
    /// for the trilinear functions, and its sum on each component is the mass of the cell.
    #[test]
    fn the_mass_is_the_integral_of_the_corner_functions() {
        let m = mass(SIZE, 7.0);
        let jac = SIZE[0] * SIZE[1] * SIZE[2] / 8.0;
        for i in 0..8 {
            for j in 0..8 {
                let exact: f64 = gauss()
                    .iter()
                    .map(|&xi| 7.0 * shape(xi)[i] * shape(xi)[j] * jac)
                    .sum();
                assert!((m[(3 * i) * 24 + 3 * j] - exact).abs() < 1e-14);
                assert_eq!(m[(3 * i) * 24 + 3 * j + 1], 0.0);
            }
        }
        let total: f64 = (0..24)
            .flat_map(|r| (0..24).map(move |c| (r, c)))
            .map(|(r, c)| m[r * 24 + c])
            .sum();
        assert!((total - 3.0 * 7.0 * 8.0 * jac).abs() < 1e-12);
    }

    fn steel() -> Stiffness {
        Orthotropic::isotropic(200.0, 0.3).stiffness().unwrap()
    }

    /// The corner displacements of the field `f` of the position.
    fn nodal(f: impl Fn([f64; 3]) -> [f64; 3]) -> [f64; 24] {
        let mut u = [0.0; 24];
        for (n, b) in BITS.iter().enumerate() {
            let x: [f64; 3] = std::array::from_fn(|d| b[d] as f64 * SIZE[d]);
            u[3 * n..3 * n + 3].copy_from_slice(&f(x));
        }
        u
    }

    #[test]
    fn the_stiffness_is_symmetric_and_a_rigid_motion_has_no_force() {
        let h = Hex::new(SIZE, &steel(), [1e-5; 3]);
        for r in 0..24 {
            for c in 0..24 {
                let (a, b) = (h.k[r * 24 + c], h.k[c * 24 + r]);
                assert!((a - b).abs() <= 1e-12 * a.abs().max(1.0), "{r} {c}");
            }
        }
        // 3 translations and 3 rotations.
        let motions: [&dyn Fn([f64; 3]) -> [f64; 3]; 6] = [
            &|_| [1.0, 0.0, 0.0],
            &|_| [0.0, 1.0, 0.0],
            &|_| [0.0, 0.0, 1.0],
            &|x| [-x[1], x[0], 0.0],
            &|x| [0.0, -x[2], x[1]],
            &|x| [x[2], 0.0, -x[0]],
        ];
        let scale = h.k.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        for m in motions {
            let u = nodal(m);
            for r in 0..24 {
                let f: f64 = (0..24).map(|c| h.k[r * 24 + c] * u[c]).sum();
                assert!(f.abs() < 1e-12 * scale, "{f}");
            }
            let (e, _) = h.strain_stress([0.3, -0.2, 0.7], &u, &[0.0; 8]);
            assert!(e.iter().all(|v| v.abs() < 1e-12), "{e:?}");
        }
    }

    /// A uniform strain field gives the uniform stress D e at each point (the patch test of 1
    /// cell), and a uniform thermal strain with no displacement gives -D alpha dT.
    #[test]
    fn a_uniform_strain_gives_the_exact_stress() {
        let d = steel();
        let h = Hex::new(SIZE, &d, [1e-5, 2e-5, 3e-5]);
        let e0 = [1e-3, -2e-3, 0.5e-3, 4e-4, -3e-4, 2e-4];
        let u = nodal(|x| {
            [
                e0[0] * x[0] + e0[5] * x[1],
                e0[1] * x[1] + e0[3] * x[2],
                e0[2] * x[2] + e0[4] * x[0],
            ]
        });
        let want = d.stress(&e0);
        for xi in gauss() {
            let (e, s) = h.strain_stress(xi, &u, &[0.0; 8]);
            for i in 0..6 {
                assert!((e[i] - e0[i]).abs() < 1e-15, "{e:?}");
                assert!((s[i] - want[i]).abs() < 1e-12, "{s:?} {want:?}");
            }
        }
        let (_, s) = h.strain_stress([0.1, 0.2, 0.3], &[0.0; 24], &[10.0; 8]);
        let alpha = [1e-5, 2e-5, 3e-5];
        for i in 0..3 {
            let want: f64 = -(0..3)
                .map(|j| d.normal[i][j] * alpha[j] * 10.0)
                .sum::<f64>();
            assert!((s[i] - want).abs() < 1e-12, "{s:?}");
        }
    }

    /// A pure bending moment on 1 cell: the corner forces of a linear stress through z. The
    /// incompatible modes give the exact curvature, the plain hex gives far less.
    #[test]
    fn one_cell_bends_with_the_exact_curvature() {
        let e = 1000.0;
        let d = Orthotropic::isotropic(e, 0.0).stiffness().unwrap();
        let h = Hex::new(SIZE, &d, [0.0; 3]);
        // sigma_xx = -E kappa (z - c/2) on the x faces, as corner forces. The exact solution has
        // u_x = -kappa x (z - c/2) and u_z = kappa x^2 / 2.
        let kappa = 1e-3;
        let u = nodal(|x| {
            [
                -kappa * x[0] * (x[2] - SIZE[2] / 2.0),
                0.0,
                kappa * x[0] * x[0] / 2.0,
            ]
        });
        // The bending energy of the exact field, E I kappa^2 L / 2, equals u^T K u / 2 when the
        // cell holds the bending exactly.
        let ku: f64 = (0..24)
            .map(|r| (0..24).map(|c| h.k[r * 24 + c] * u[c]).sum::<f64>() * u[r])
            .sum();
        let i = SIZE[1] * SIZE[2].powi(3) / 12.0;
        let exact = e * i * kappa * kappa * SIZE[0];
        assert!((ku - exact).abs() < 1e-9 * exact, "{ku} {exact}");
    }

    #[test]
    fn the_orthotropic_stiffness_inverts_the_compliance() {
        let m = Orthotropic {
            e: [140.0, 10.0, 10.0],
            nu: [0.3, 0.3, 0.45],
            g: [5.0, 5.0, 3.5],
        };
        let c = m.stiffness().unwrap().normal;
        let s = [
            [1.0 / 140.0, -0.3 / 140.0, -0.3 / 140.0],
            [-0.3 / 140.0, 0.1, -0.045],
            [-0.3 / 140.0, -0.045, 0.1],
        ];
        for i in 0..3 {
            for j in 0..3 {
                let v: f64 = (0..3).map(|k| c[i][k] * s[k][j]).sum();
                assert!((v - f64::from(u8::from(i == j))).abs() < 1e-12);
            }
        }
        assert!(Orthotropic::isotropic(1.0, 0.5).stiffness().is_err());
    }
}
