//! The iterative solver of the edge element matrices of `em` for a model too large for the direct
//! factor: GMRES with a right preconditioner that approximates the inverse of the shifted
//! operator P = S + k0^2 M (curl-curl plus mass, not indefinite), where the system is
//! A = S - k0^2 M (indefinite).
//!
//! The preconditioner is the nodal auxiliary space method: a symmetric Gauss-Seidel smoother on
//! P, and between its 2 sweeps the corrections in 2 nodal spaces, each solved exactly by the
//! direct LDL^T factor of its nodal matrix:
//! - the vector space of continuous linear fields, through the interpolation Pi to the edges
//!   (the edge value is the mean of the 2 point values along the edge), with the vector
//!   Laplacian of nu plus the mass of k0^2 eps;
//! - the gradients of the continuous linear potentials, through the discrete gradient G, with the
//!   matrix k0^2 G^T M G (curl G = 0, so this is G^T P G with no face terms).
//!
//! A nodal matrix has 1 unknown for each point, against about 6 edge unknowns for each point, so
//! its factor is much smaller than the factor of the edge matrix.
//!
//! Sources: R. Hiptmair and J. Xu, Nodal auxiliary space preconditioning in H(curl) and H(div)
//! spaces, SIAM J. Numer. Anal. 45 (2007) 2483-2509 (the auxiliary spaces and the smoother);
//! Y. A. Erlangga, C. W. Oosterlee and C. Vuik, A novel multigrid based preconditioner for
//! heterogeneous Helmholtz problems, SIAM J. Sci. Comput. 27 (2006) 1471-1492 (a preconditioner
//! from a shifted operator that is not indefinite); Y. Saad and M. H. Schultz, GMRES: a
//! generalized minimal residual algorithm for solving nonsymmetric linear systems, SIAM J. Sci.
//! Stat. Comput. 7 (1986) 856-869, and Y. Saad, Iterative Methods for Sparse Linear Systems, 2nd
//! ed., SIAM 2003, sections 6.5 and 9.3 (restarted GMRES with a right preconditioner, with
//! Givens rotations in complex arithmetic).

use crate::complex::C64;
use crate::ldlt::Ldlt;
use crate::sparse::Zcsr;

/// An edge unknown in the auxiliary spaces: its 2 points (the smaller global point first) in the
/// numbering of the vector space and of the gradient space, `u32::MAX` for a point that is not
/// in the space, and half the edge vector.
pub struct Edge {
    pub unknown: u32,
    pub vector: [u32; 2],
    pub gradient: [u32; 2],
    pub half: [f64; 3],
}

/// The preconditioner.
pub struct Auxiliary {
    /// The shifted operator P on the edges.
    pub p: Zcsr,
    /// The position of the diagonal of each row of `p`.
    diag: Vec<usize>,
    edges: Vec<Edge>,
    /// The factor of the vector Laplacian (1 component) and of the gradient matrix.
    vector: Ldlt,
    gradient: Ldlt,
    /// The number of points of the vector space and of the gradient space.
    vector_points: usize,
    gradient_points: usize,
}

impl Auxiliary {
    pub fn new(
        p: Zcsr,
        edges: Vec<Edge>,
        vector: Ldlt,
        gradient: Ldlt,
        vector_points: usize,
        gradient_points: usize,
    ) -> Auxiliary {
        let pat = &p.pattern;
        let diag = (0..p.n())
            .map(|i| {
                (pat.ptr[i]..pat.ptr[i + 1])
                    .find(|&k| pat.idx[k] as usize == i)
                    .expect("each row has its diagonal")
            })
            .collect();
        Auxiliary {
            p,
            diag,
            edges,
            vector,
            gradient,
            vector_points,
            gradient_points,
        }
    }

    /// z = B r, the approximate inverse of P.
    pub fn apply(&self, r: &[C64], z: &mut [C64]) {
        let n = r.len();
        z.fill(C64::ZERO);
        self.sweep(r, z, false);
        let mut res = vec![C64::ZERO; n];
        self.residual(r, z, &mut res);
        // The corrections in the 2 nodal spaces, from 1 residual.
        let np = self.vector_points;
        let mut v = vec![C64::ZERO; 3 * np];
        let mut s = vec![C64::ZERO; self.gradient_points];
        for e in &self.edges {
            let w = res[e.unknown as usize];
            for q in e.vector {
                if q != u32::MAX {
                    for c in 0..3 {
                        v[c * np + q as usize] += w * e.half[c];
                    }
                }
            }
            for (q, sign) in e.gradient.into_iter().zip([-1.0, 1.0]) {
                if q != u32::MAX {
                    s[q as usize] += w * sign;
                }
            }
        }
        for c in 0..3 {
            self.vector.solve(&mut v[c * np..(c + 1) * np]);
        }
        self.gradient.solve(&mut s);
        for e in &self.edges {
            let mut w = C64::ZERO;
            for q in e.vector {
                if q != u32::MAX {
                    for c in 0..3 {
                        w += v[c * np + q as usize] * e.half[c];
                    }
                }
            }
            for (q, sign) in e.gradient.into_iter().zip([-1.0, 1.0]) {
                if q != u32::MAX {
                    w += s[q as usize] * sign;
                }
            }
            z[e.unknown as usize] += w;
        }
        self.residual(r, z, &mut res);
        let mut dz = vec![C64::ZERO; n];
        self.sweep(&res, &mut dz, true);
        for (a, b) in z.iter_mut().zip(&dz) {
            *a += *b;
        }
    }

    /// res = r - P z.
    fn residual(&self, r: &[C64], z: &[C64], res: &mut [C64]) {
        self.p.mul(z, res);
        for (a, b) in res.iter_mut().zip(r) {
            *a = *b - *a;
        }
    }

    /// 1 Gauss-Seidel sweep on P z = r, forward or backward.
    fn sweep(&self, r: &[C64], z: &mut [C64], backward: bool) {
        let pat = &self.p.pattern;
        let n = r.len();
        for k in 0..n {
            let i = if backward { n - 1 - k } else { k };
            let mut sum = r[i];
            for e in pat.ptr[i]..pat.ptr[i + 1] {
                let j = pat.idx[e] as usize;
                if j != i {
                    sum -= self.p.val[e] * z[j];
                }
            }
            z[i] = sum / self.p.val[self.diag[i]];
        }
    }
}

/// The Hermitian inner product.
fn inner(a: &[C64], b: &[C64]) -> C64 {
    a.iter()
        .zip(b)
        .fold(C64::ZERO, |s, (x, y)| s + x.conj() * *y)
}

fn norm(a: &[C64]) -> f64 {
    a.iter().map(|x| x.norm2()).sum::<f64>().sqrt()
}

/// The settings of GMRES: the restart length, the residual relative to the right side, and the
/// most iterations.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub restart: usize,
    pub tolerance: f64,
    pub iterations: usize,
}

/// Solve A x = b by GMRES with the right preconditioner `b_inv`, from the start `x`. The answer
/// is the number of iterations. `stop` is checked at each restart.
pub fn gmres(
    a: &Zcsr,
    b: &[C64],
    x: &mut [C64],
    b_inv: &dyn Fn(&[C64], &mut [C64]),
    settings: Settings,
    stop: &mut dyn FnMut() -> bool,
) -> Result<usize, String> {
    let Settings {
        restart,
        tolerance: tol,
        iterations: max_iterations,
    } = settings;
    let n = b.len();
    let bnorm = norm(b);
    if bnorm == 0.0 {
        x.fill(C64::ZERO);
        return Ok(0);
    }
    let mut done = 0;
    let mut w = vec![C64::ZERO; n];
    let mut t = vec![C64::ZERO; n];
    loop {
        if stop() {
            return Err("cancelled".into());
        }
        // r = b - A x.
        a.mul(x, &mut w);
        let r: Vec<C64> = b.iter().zip(&w).map(|(p, q)| *p - *q).collect();
        let beta = norm(&r);
        if beta <= tol * bnorm {
            return Ok(done);
        }
        if done >= max_iterations {
            return Err(format!(
                "GMRES did not converge in {max_iterations} iterations: the residual is {:.1e} \
                 of the right side",
                beta / bnorm
            ));
        }
        let mut v: Vec<Vec<C64>> = vec![r.iter().map(|z| *z / beta).collect()];
        // The Hessenberg matrix by columns, the rotations and the right side of the small
        // least squares problem.
        let mut h: Vec<Vec<C64>> = Vec::new();
        let mut rot: Vec<(f64, C64)> = Vec::new();
        let mut g = vec![C64::from(beta)];
        for j in 0..restart {
            b_inv(&v[j], &mut t);
            a.mul(&t, &mut w);
            let mut col = Vec::with_capacity(j + 2);
            for vi in &v {
                let hij = inner(vi, &w);
                for (p, q) in w.iter_mut().zip(vi) {
                    *p -= hij * *q;
                }
                col.push(hij);
            }
            let hn = norm(&w);
            col.push(C64::from(hn));
            for (i, &(c, s)) in rot.iter().enumerate() {
                let (p, q) = (col[i], col[i + 1]);
                col[i] = p * c + s * q;
                col[i + 1] = q * c - s.conj() * p;
            }
            let (p, q) = (col[j], col[j + 1]);
            let r = (p.norm2() + q.norm2()).sqrt();
            let (c, s) = if p.abs() == 0.0 {
                (0.0, C64::ONE)
            } else {
                (p.abs() / r, p / p.abs() * q.conj() / r)
            };
            col[j] = p * c + s * q;
            col[j + 1] = C64::ZERO;
            rot.push((c, s));
            let gj = g[j];
            g[j] = gj * c;
            g.push(-(s.conj() * gj));
            h.push(col);
            done += 1;
            let converged = g[j + 1].abs() <= tol * bnorm;
            if converged || j + 1 == restart || done >= max_iterations || hn == 0.0 {
                break;
            }
            v.push(w.iter().map(|z| *z / hn).collect());
        }
        // y = H^-1 g, then x += B^-1 (V y).
        let m = h.len();
        let mut y = vec![C64::ZERO; m];
        for i in (0..m).rev() {
            let mut s = g[i];
            for k in i + 1..m {
                s -= h[k][i] * y[k];
            }
            y[i] = s / h[i][i];
        }
        let mut u = vec![C64::ZERO; n];
        for (vk, yk) in v.iter().zip(&y) {
            for (p, q) in u.iter_mut().zip(vk) {
                *p += *q * *yk;
            }
        }
        b_inv(&u, &mut t);
        for (p, q) in x.iter_mut().zip(&t) {
            *p += *q;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse::Csr;

    const SETTINGS: Settings = Settings {
        restart: 30,
        tolerance: 1e-10,
        iterations: 2000,
    };

    #[test]
    fn gmres_solves_a_nonsymmetric_complex_matrix() {
        // A tridiagonal complex matrix with a Jacobi preconditioner.
        let n = 200;
        let elements: Vec<[u32; 2]> = (0..n as u32 - 1).map(|i| [i, i + 1]).collect();
        let mut a = Zcsr::zeros(Csr::pattern(n, elements.iter().map(|e| &e[..])));
        for e in &elements {
            a.add_element(
                e,
                &[
                    C64::new(1.0, 0.1),
                    C64::new(-1.0, 0.0),
                    C64::new(-0.5, 0.2),
                    C64::new(1.0, 0.1),
                ],
            );
        }
        for i in 0..n as u32 {
            a.add_element(&[i], &[C64::new(0.5, 0.3)]);
        }
        let x0: Vec<C64> = (0..n).map(|i| C64::new((i as f64).sin(), 1.0)).collect();
        let mut b = vec![C64::ZERO; n];
        a.mul(&x0, &mut b);
        let mut x = vec![C64::ZERO; n];
        let it = gmres(
            &a,
            &b,
            &mut x,
            &|r, z| z.copy_from_slice(r),
            SETTINGS,
            &mut || false,
        )
        .unwrap();
        let err = x
            .iter()
            .zip(&x0)
            .map(|(p, q)| (*p - *q).abs())
            .fold(0.0, f64::max);
        assert!(err < 1e-7, "error {err} after {it} iterations");
    }
}
