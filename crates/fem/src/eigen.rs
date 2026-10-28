//! The real symmetric eigensolver: the lowest eigenpairs of K x = lambda M x, with K symmetric
//! and M symmetric positive definite, by shift-invert subspace iteration with Rayleigh-Ritz, and
//! the dense symmetric eigenproblems of the small projected matrices.
//!
//! The shift sigma is below the lowest eigenvalue, so K - sigma M is positive definite also when
//! K is singular (a body with rigid motions). The caller gives the solve of (K - sigma M) x = b,
//! the direct factor or PCG. Each iteration solves for p vectors at once and projects K and M
//! on them; the eigenvectors of the small p x p problem give the next p vectors. A vector has
//! converged when the angle in the M inner product between it and its image under
//! (K - sigma M)^-1 M is below `TOL`: an exact eigenvector has the same direction as its image.
//! The subspace has more vectors than the requested count, p = max(2 q, q + 8), so a cluster or
//! a repeated eigenvalue converges with each of its vectors.
//!
//! The small problem K_r q = lambda M_r q scales M_r to a unit diagonal, reduces to a standard
//! problem with the Cholesky factor of M_r, and solves that with the cyclic Jacobi method.
//!
//! The iteration finds the eigenvalues nearest to the shift when the start vectors are not
//! orthogonal to their eigenvectors. Pseudo-random start vectors make that a certainty in
//! practice; there is no Sturm sequence check.
//!
//! Sources: K.-J. Bathe, The subspace iteration method - revisited, Computers & Structures 126
//! (2013) 177-183 (the iteration, the subspace size and the convergence measure); Y. Saad,
//! Numerical Methods for Large Eigenvalue Problems, 2nd ed., SIAM 2011, chapter 5 (subspace
//! iteration with projection) and section 4.3 (Rayleigh-Ritz); G. H. Golub and C. F. Van Loan,
//! Matrix Computations, 4th ed., 2013, sections 8.5 (the cyclic Jacobi method) and 8.7 (the
//! symmetric-definite problem by Cholesky).

use crate::sparse::Csr;

/// The largest angle (its sine, in the M inner product) between a converged vector and its
/// image. Reason: the eigenvalue error is about the square of the angle, so 1e-6 gives the
/// eigenvalue to about 1e-12 of the spread of the spectrum, and the angle that the rounding of
/// the inner products leaves is about 1e-8.
pub const TOL: f64 = 1e-6;

/// The most iterations. Reason: the angle of vector i falls by about (lambda_i - sigma) /
/// (lambda_p+1 - sigma) in each iteration, at most about 0.75 for the last requested vector
/// with p = q + 8 on the spectrum of a beam, so 60 iterations reach 1e-6 from 1 with a margin
/// of 2.
pub const MAX_ITER: usize = 120;

/// The most memory of the subspace vectors (bytes). Reason: the iteration keeps 3 sets of p
/// vectors of length n, so 100 modes of a stress grid at its limit of 300,000 points need
/// 4.3 GB on top of the factor; 2 GB is the limit of the direct factor of flat cells.
pub const MAX_BYTES: f64 = 2e9;

/// The count of subspace vectors for `count` modes of `n` unknowns.
fn vectors(n: usize, count: usize) -> usize {
    (2 * count).max(count + 8).min(n)
}

/// An error when the subspace vectors of `count` modes of `n` unknowns need more than
/// `MAX_BYTES`.
pub fn fits(n: usize, count: usize) -> Result<(), String> {
    let bytes = 3.0 * vectors(n, count) as f64 * n as f64 * 8.0;
    if bytes > MAX_BYTES {
        return Err(format!(
            "{count} modes of {n} unknowns need {:.1} GB for the subspace vectors, more than {:.0} GB: ask for fewer modes, or use CalculiX *FREQUENCY",
            bytes / 1e9,
            MAX_BYTES / 1e9
        ));
    }
    Ok(())
}

/// The eigenpairs, the lowest eigenvalue first.
#[derive(Debug, Clone, PartialEq)]
pub struct Pairs {
    pub values: Vec<f64>,
    /// Each vector has x^T M x = 1.
    pub vectors: Vec<Vec<f64>>,
    pub iterations: usize,
}

/// A linear solve: `solve(b, x)` sets x to the solution for the right-hand side b.
pub type Solve<'a> = dyn FnMut(&[f64], &mut [f64]) -> Result<(), String> + 'a;

/// The `count` lowest eigenpairs of K x = lambda M x by subspace iteration with the shift
/// `shift`. `solve(b, x)` sets x to (K - shift M)^-1 b; x holds a start value for an iterative
/// solve.
pub fn subspace(
    k: &Csr,
    m: &Csr,
    shift: f64,
    count: usize,
    solve: &mut Solve,
    cancelled: &dyn Fn() -> bool,
) -> Result<Pairs, String> {
    let n = k.n;
    if count == 0 || count > n {
        return Err(format!(
            "the model has {n} unknowns, so it has 1 to {n} modes, not {count}"
        ));
    }
    let p = vectors(n, count);
    // The start vectors: the diagonal of M, and pseudo-random vectors (xorshift).
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let mut x: Vec<Vec<f64>> = (0..p)
        .map(|j| {
            if j == 0 {
                m.diagonal()
            } else {
                (0..n).map(|_| random()).collect()
            }
        })
        .collect();
    let mut y = vec![vec![0.0; n]; p];
    // The shifted Ritz values of the last iteration.
    let mut mu: Vec<f64> = Vec::new();
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(u, v)| u * v).sum::<f64>();
    for iteration in 1..=MAX_ITER {
        if cancelled() {
            return Err("cancelled".into());
        }
        // Y = M X and X_bar = (K - shift M)^-1 Y.
        let mut bar = Vec::with_capacity(p);
        for j in 0..p {
            m.mul(&x[j], &mut y[j]);
            // The start value: the image x / mu of an exact eigenvector, after an iteration.
            let mut b = match mu.get(j) {
                Some(&u) => x[j].iter().map(|v| v / u).collect(),
                None => vec![0.0; n],
            };
            solve(&y[j], &mut b)?;
            bar.push(b);
        }
        // The projections: K_r = X_bar^T (K - shift M) X_bar = X_bar^T Y, M_r = X_bar^T M X_bar.
        let mut kr = vec![0.0; p * p];
        let mut mr = vec![0.0; p * p];
        let mut mb = vec![0.0; n];
        for j in 0..p {
            m.mul(&bar[j], &mut mb);
            for i in 0..=j {
                let kv = 0.5 * (dot(&bar[i], &y[j]) + dot(&bar[j], &y[i]));
                let mv = dot(&bar[i], &mb);
                kr[i * p + j] = kv;
                kr[j * p + i] = kv;
                mr[i * p + j] = mv;
                mr[j * p + i] = mv;
            }
        }
        // X was M-orthonormal after an iteration: the angle of vector i to its image.
        let converged = iteration > 1
            && (0..count).all(|i| {
                let (a, b) = (kr[i * p + i], mr[i * p + i]);
                (1.0 - a * a / b).max(0.0).sqrt() <= TOL
            });
        let (values, q) = generalized(&kr, &mr, p)?;
        mu = values;
        for j in 0..p {
            let xj = &mut x[j];
            xj.iter_mut().for_each(|v| *v = 0.0);
            for (i, b) in bar.iter().enumerate() {
                let c = q[i * p + j];
                for (v, w) in xj.iter_mut().zip(b) {
                    *v += c * w;
                }
            }
        }
        if converged {
            x.truncate(count);
            return Ok(Pairs {
                values: mu[..count].iter().map(|v| v + shift).collect(),
                vectors: x,
                iterations: iteration,
            });
        }
    }
    Err(format!(
        "the subspace iteration did not converge in {MAX_ITER} iterations: ask for fewer modes"
    ))
}

/// The eigenpairs of K q = lambda M q of 2 dense symmetric n x n matrices (row-major), M
/// positive definite: the eigenvalues in ascending order and the vectors as the columns of an
/// n x n matrix (row-major), with Q^T M Q = I.
pub fn generalized(k: &[f64], m: &[f64], n: usize) -> Result<(Vec<f64>, Vec<f64>), String> {
    // D M D has a unit diagonal, which keeps the factor accurate when the columns differ in
    // scale.
    let mut d = vec![0.0; n];
    for i in 0..n {
        if !(m[i * n + i] > 0.0) {
            return Err("the projected mass matrix is not positive definite".into());
        }
        d[i] = 1.0 / m[i * n + i].sqrt();
    }
    // The Cholesky factor L of D M D (lower triangle, row-major).
    let mut l = vec![0.0; n * n];
    for j in 0..n {
        let mut s = m[j * n + j] * d[j] * d[j];
        for c in 0..j {
            s -= l[j * n + c] * l[j * n + c];
        }
        if !(s > 1e-14) {
            return Err(
                "the projected mass matrix is singular: the subspace vectors are dependent".into(),
            );
        }
        let s = s.sqrt();
        l[j * n + j] = s;
        for i in j + 1..n {
            let mut v = m[i * n + j] * d[i] * d[j];
            for c in 0..j {
                v -= l[i * n + c] * l[j * n + c];
            }
            l[i * n + j] = v / s;
        }
    }
    // C = L^-1 (D K D) L^-T: first W = L^-1 (D K D), then C = L^-1 W^T.
    let lower_solve = |b: &mut [f64]| {
        for i in 0..n {
            let mut v = b[i];
            for c in 0..i {
                v -= l[i * n + c] * b[c];
            }
            b[i] = v / l[i * n + i];
        }
    };
    let mut w = vec![0.0; n * n];
    let mut col = vec![0.0; n];
    for j in 0..n {
        for i in 0..n {
            col[i] = k[i * n + j] * d[i] * d[j];
        }
        lower_solve(&mut col);
        for i in 0..n {
            w[i * n + j] = col[i];
        }
    }
    let mut c = vec![0.0; n * n];
    for j in 0..n {
        col.copy_from_slice(&w[j * n..(j + 1) * n]);
        lower_solve(&mut col);
        for i in 0..n {
            c[i * n + j] = col[i];
        }
    }
    for i in 0..n {
        for j in 0..i {
            let v = 0.5 * (c[i * n + j] + c[j * n + i]);
            c[i * n + j] = v;
            c[j * n + i] = v;
        }
    }
    let (values, v) = jacobi(&mut c, n);
    // Q = D L^-T V.
    let mut q = vec![0.0; n * n];
    for j in 0..n {
        for i in (0..n).rev() {
            let mut s = v[i * n + j];
            for r in i + 1..n {
                s -= l[r * n + i] * q[r * n + j];
            }
            q[i * n + j] = s / l[i * n + i];
        }
    }
    for i in 0..n {
        for j in 0..n {
            q[i * n + j] *= d[i];
        }
    }
    Ok((values, q))
}

/// The eigenvalues in ascending order and the orthonormal eigenvectors (the columns of an n x n
/// matrix, row-major) of the dense symmetric matrix `a` (row-major), which the cyclic Jacobi
/// method makes diagonal.
pub fn jacobi(a: &mut [f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0.0; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let norm = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    for _sweep in 0..100 {
        let off: f64 = (0..n)
            .flat_map(|i| (0..n).filter(move |&j| j != i).map(move |j| (i, j)))
            .map(|(i, j)| a[i * n + j] * a[i * n + j])
            .sum::<f64>()
            .sqrt();
        if !(off > 1e-15 * norm) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq == 0.0 {
                    continue;
                }
                // The rotation that sets a_pq to 0 (Golub and Van Loan, algorithm 8.5.1).
                let tau = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = if tau >= 0.0 {
                    1.0 / (tau + tau.hypot(1.0))
                } else {
                    -1.0 / (-tau + tau.hypot(1.0))
                };
                let c = 1.0 / t.hypot(1.0);
                let s = t * c;
                for r in 0..n {
                    let (x, y) = (a[r * n + p], a[r * n + q]);
                    a[r * n + p] = c * x - s * y;
                    a[r * n + q] = s * x + c * y;
                }
                for r in 0..n {
                    let (x, y) = (a[p * n + r], a[q * n + r]);
                    a[p * n + r] = c * x - s * y;
                    a[q * n + r] = s * x + c * y;
                }
                for r in 0..n {
                    let (x, y) = (v[r * n + p], v[r * n + q]);
                    v[r * n + p] = c * x - s * y;
                    v[r * n + q] = s * x + c * y;
                }
            }
        }
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| a[i * n + i].total_cmp(&a[j * n + j]));
    let values = order.iter().map(|&i| a[i * n + i]).collect();
    let mut sorted = vec![0.0; n * n];
    for (new, &old) in order.iter().enumerate() {
        for r in 0..n {
            sorted[r * n + new] = v[r * n + old];
        }
    }
    (values, sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Jacobi method gives the eigenpairs of a small matrix with known eigenvalues.
    #[test]
    fn jacobi_finds_the_eigenvalues_of_the_second_difference() {
        // The second difference matrix of order n has eigenvalues 2 - 2 cos(k pi / (n + 1)).
        let n = 7;
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            a[i * n + i] = 2.0;
            if i + 1 < n {
                a[i * n + i + 1] = -1.0;
                a[(i + 1) * n + i] = -1.0;
            }
        }
        let orig = a.clone();
        let (values, v) = jacobi(&mut a, n);
        for k in 0..n {
            let exact = 2.0 - 2.0 * ((k + 1) as f64 * std::f64::consts::PI / (n + 1) as f64).cos();
            assert!((values[k] - exact).abs() < 1e-13, "{} {exact}", values[k]);
            // A v = lambda v.
            for i in 0..n {
                let av: f64 = (0..n).map(|j| orig[i * n + j] * v[j * n + k]).sum();
                assert!((av - values[k] * v[i * n + k]).abs() < 1e-12);
            }
        }
    }

    /// The generalized problem gives M-orthonormal vectors and K q = lambda M q.
    #[test]
    fn generalized_gives_m_orthonormal_vectors() {
        let n = 5;
        let mut k = vec![0.0; n * n];
        let mut m = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                let (a, b) = (i as f64, j as f64);
                k[i * n + j] = 1.0 / (1.0 + a + b) + if i == j { 3.0 } else { 0.0 };
                m[i * n + j] = if i == j {
                    1e4f64.powi(i as i32 % 2)
                } else {
                    0.1
                };
            }
        }
        let (values, q) = generalized(&k, &m, n).unwrap();
        for a in 0..n {
            for b in 0..n {
                let qmq: f64 = (0..n)
                    .flat_map(|i| (0..n).map(move |j| (i, j)))
                    .map(|(i, j)| q[i * n + a] * m[i * n + j] * q[j * n + b])
                    .sum();
                assert!(
                    (qmq - f64::from(u8::from(a == b))).abs() < 1e-10,
                    "{a} {b} {qmq}"
                );
            }
            for i in 0..n {
                let kq: f64 = (0..n).map(|j| k[i * n + j] * q[j * n + a]).sum();
                let mq: f64 = (0..n).map(|j| m[i * n + j] * q[j * n + a]).sum();
                assert!((kq - values[a] * mq).abs() < 1e-9 * kq.abs().max(1e-3));
            }
        }
        assert!(values.windows(2).all(|w| w[0] <= w[1]));
    }

    /// Subspace iteration finds the lowest modes of a free chain of springs and masses,
    /// including its 0 mode, with a shift below 0.
    #[test]
    fn subspace_finds_the_modes_of_a_free_chain() {
        // A free chain of n unit masses and unit springs: lambda_k = 2 - 2 cos(k pi / n).
        let n = 40;
        let mut k = Csr::pattern(
            n,
            (0..n - 1)
                .map(|i| [i as u32, i as u32 + 1])
                .collect::<Vec<_>>()
                .iter()
                .map(|e| &e[..]),
        );
        let mut m = k.clone();
        for i in 0..n - 1 {
            k.add_element(&[i as u32, i as u32 + 1], &[1.0, -1.0, -1.0, 1.0]);
        }
        for i in 0..n {
            m.add_element(&[i as u32], &[1.0]);
        }
        let shift = -0.01;
        // The dense inverse of K - shift M.
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                let kij = k.find(i, j).map_or(0.0, |p| k.val[p]);
                a[i * n + j] = kij - shift * f64::from(u8::from(i == j));
            }
        }
        let (values, v) = jacobi(&mut a, n);
        let mut solve = |b: &[f64], x: &mut [f64]| {
            let c: Vec<f64> = (0..n)
                .map(|r| (0..n).map(|i| v[i * n + r] * b[i]).sum::<f64>() / values[r])
                .collect();
            for i in 0..n {
                x[i] = (0..n).map(|r| v[i * n + r] * c[r]).sum();
            }
            Ok(())
        };
        let count = 6;
        let pairs = subspace(&k, &m, shift, count, &mut solve, &|| false).unwrap();
        for j in 0..count {
            let exact = 2.0 - 2.0 * (j as f64 * std::f64::consts::PI / n as f64).cos();
            assert!(
                (pairs.values[j] - exact).abs() < 1e-10,
                "mode {j}: {} against {exact}",
                pairs.values[j]
            );
        }
    }
}
