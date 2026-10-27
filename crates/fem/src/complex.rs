//! A complex number of 2 `f64`, and the dense complex matrix algebra of the transmission line
//! model: the product, the LU solve with partial pivots, the exponential and the eigenvalues.
//!
//! Sources: G. H. Golub and C. F. Van Loan, Matrix Computations, 4th ed., 2013, section 3.4
//! (Gaussian elimination with partial pivots), section 7.4 (the Householder reduction to
//! Hessenberg form), section 7.5 (the shifted QR iteration, Wilkinson shift and deflation) and
//! section 9.3 (the [q/q] Pade approximant of the exponential after scaling to a norm of 1/2 or
//! less, then the squarings).

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    pub const ZERO: C64 = C64 { re: 0.0, im: 0.0 };
    pub const ONE: C64 = C64 { re: 1.0, im: 0.0 };

    pub const fn new(re: f64, im: f64) -> C64 {
        C64 { re, im }
    }

    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }

    pub fn conj(self) -> C64 {
        C64::new(self.re, -self.im)
    }

    /// The square root with a real part of 0 or more.
    pub fn sqrt(self) -> C64 {
        let r = self.abs();
        if r == 0.0 {
            return C64::ZERO;
        }
        let re = ((r + self.re) / 2.0).sqrt();
        let im = ((r - self.re) / 2.0).sqrt();
        C64::new(re, if self.im < 0.0 { -im } else { im })
    }

    pub fn scale(self, s: f64) -> C64 {
        C64::new(self.re * s, self.im * s)
    }
}

impl From<f64> for C64 {
    fn from(re: f64) -> C64 {
        C64::new(re, 0.0)
    }
}

impl Add for C64 {
    type Output = C64;
    fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
}

impl Sub for C64 {
    type Output = C64;
    fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
}

impl Mul for C64 {
    type Output = C64;
    fn mul(self, o: C64) -> C64 {
        C64::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}

impl Div for C64 {
    type Output = C64;
    fn div(self, o: C64) -> C64 {
        // Smith's method, so that a large or a small divisor does not overflow.
        if o.re.abs() >= o.im.abs() {
            let r = o.im / o.re;
            let d = o.re + o.im * r;
            C64::new((self.re + self.im * r) / d, (self.im - self.re * r) / d)
        } else {
            let r = o.re / o.im;
            let d = o.re * r + o.im;
            C64::new((self.re * r + self.im) / d, (self.im * r - self.re) / d)
        }
    }
}

impl Neg for C64 {
    type Output = C64;
    fn neg(self) -> C64 {
        C64::new(-self.re, -self.im)
    }
}

impl AddAssign for C64 {
    fn add_assign(&mut self, o: C64) {
        *self = *self + o;
    }
}

impl SubAssign for C64 {
    fn sub_assign(&mut self, o: C64) {
        *self = *self - o;
    }
}

impl MulAssign for C64 {
    fn mul_assign(&mut self, o: C64) {
        *self = *self * o;
    }
}

/// A dense square complex matrix, row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Matrix {
    pub n: usize,
    pub a: Vec<C64>,
}

impl Matrix {
    pub fn zeros(n: usize) -> Matrix {
        Matrix {
            n,
            a: vec![C64::ZERO; n * n],
        }
    }

    pub fn identity(n: usize) -> Matrix {
        let mut m = Matrix::zeros(n);
        for i in 0..n {
            m.a[i * n + i] = C64::ONE;
        }
        m
    }

    pub fn at(&self, i: usize, j: usize) -> C64 {
        self.a[i * self.n + j]
    }

    pub fn set(&mut self, i: usize, j: usize, v: C64) {
        self.a[i * self.n + j] = v;
    }

    pub fn mul(&self, o: &Matrix) -> Matrix {
        let n = self.n;
        let mut c = Matrix::zeros(n);
        for i in 0..n {
            for k in 0..n {
                let v = self.a[i * n + k];
                if v == C64::ZERO {
                    continue;
                }
                for j in 0..n {
                    c.a[i * n + j] += v * o.a[k * n + j];
                }
            }
        }
        c
    }

    pub fn add(&self, o: &Matrix, s: f64) -> Matrix {
        Matrix {
            n: self.n,
            a: self
                .a
                .iter()
                .zip(&o.a)
                .map(|(x, y)| *x + y.scale(s))
                .collect(),
        }
    }

    pub fn scale(&self, s: f64) -> Matrix {
        Matrix {
            n: self.n,
            a: self.a.iter().map(|x| x.scale(s)).collect(),
        }
    }

    /// The largest sum of the absolute values of a row.
    pub fn norm_inf(&self) -> f64 {
        (0..self.n)
            .map(|i| {
                self.a[i * self.n..(i + 1) * self.n]
                    .iter()
                    .map(|v| v.abs())
                    .sum()
            })
            .fold(0.0, f64::max)
    }

    /// Solve self X = B for each column of `b` (a matrix of the same size). An error when the
    /// matrix is singular to the precision of the arithmetic.
    pub fn solve(&self, b: &Matrix) -> Result<Matrix, String> {
        let n = self.n;
        let mut a = self.a.clone();
        let mut x = b.a.clone();
        let scale = self.norm_inf();
        for k in 0..n {
            let p = (k..n)
                .max_by(|&i, &j| a[i * n + k].abs().total_cmp(&a[j * n + k].abs()))
                .expect("a row");
            if !(a[p * n + k].abs() > 1e-14 * scale) {
                return Err("the matrix is singular".into());
            }
            if p != k {
                for j in 0..n {
                    a.swap(k * n + j, p * n + j);
                    x.swap(k * n + j, p * n + j);
                }
            }
            let d = a[k * n + k];
            for i in k + 1..n {
                let f = a[i * n + k] / d;
                if f == C64::ZERO {
                    continue;
                }
                for j in k..n {
                    let v = a[k * n + j];
                    a[i * n + j] -= f * v;
                }
                for j in 0..n {
                    let v = x[k * n + j];
                    x[i * n + j] -= f * v;
                }
            }
        }
        for k in (0..n).rev() {
            for j in 0..n {
                let mut v = x[k * n + j];
                for i in k + 1..n {
                    v -= a[k * n + i] * x[i * n + j];
                }
                x[k * n + j] = v / a[k * n + k];
            }
        }
        Ok(Matrix { n, a: x })
    }

    /// The matrix exponential e^self.
    pub fn exp(&self) -> Matrix {
        // The [6/6] Pade coefficients of e^x.
        const C: [f64; 7] = [
            1.0,
            0.5,
            5.0 / 44.0,
            1.0 / 66.0,
            1.0 / 792.0,
            1.0 / 15840.0,
            1.0 / 665280.0,
        ];
        let norm = self.norm_inf();
        let squarings = if norm > 0.5 {
            (norm / 0.5).log2().ceil() as i32
        } else {
            0
        };
        let a = self.scale(0.5f64.powi(squarings));
        let id = Matrix::identity(self.n);
        let mut power = id.clone();
        let mut num = id.scale(C[0]);
        let mut den = id.scale(C[0]);
        for (k, &c) in C.iter().enumerate().skip(1) {
            power = power.mul(&a);
            num = num.add(&power, c);
            den = den.add(&power, if k % 2 == 1 { -c } else { c });
        }
        let mut e = den
            .solve(&num)
            .expect("the Pade denominator of a matrix of norm 1/2 or less is regular");
        for _ in 0..squarings {
            e = e.mul(&e);
        }
        e
    }

    /// The eigenvalues, in no special order.
    pub fn eigenvalues(&self) -> Vec<C64> {
        let n = self.n;
        let mut h = self.a.clone();
        let at = |i: usize, j: usize| i * n + j;
        // Householder reduction to upper Hessenberg form.
        for k in 0..n.saturating_sub(2) {
            let alpha: f64 = (k + 1..n)
                .map(|i| h[at(i, k)].abs().powi(2))
                .sum::<f64>()
                .sqrt();
            if alpha == 0.0 {
                continue;
            }
            let x0 = h[at(k + 1, k)];
            let phase = if x0.abs() == 0.0 {
                C64::ONE
            } else {
                x0.scale(1.0 / x0.abs())
            };
            let mut v: Vec<C64> = (k + 1..n).map(|i| h[at(i, k)]).collect();
            v[0] += phase.scale(alpha);
            let vnorm: f64 = v.iter().map(|x| x.abs().powi(2)).sum();
            // H = (I - 2 v v* / v* v) H (I - 2 v v* / v* v).
            for j in 0..n {
                let mut s = C64::ZERO;
                for (r, vr) in v.iter().enumerate() {
                    s += vr.conj() * h[at(k + 1 + r, j)];
                }
                let s = s.scale(2.0 / vnorm);
                for (r, vr) in v.iter().enumerate() {
                    h[at(k + 1 + r, j)] -= *vr * s;
                }
            }
            for i in 0..n {
                let mut s = C64::ZERO;
                for (r, vr) in v.iter().enumerate() {
                    s += h[at(i, k + 1 + r)] * *vr;
                }
                let s = s.scale(2.0 / vnorm);
                for (r, vr) in v.iter().enumerate() {
                    h[at(i, k + 1 + r)] -= s * vr.conj();
                }
            }
        }
        // Shifted QR on the active block 0..=hi, with deflation at small subdiagonals.
        let mut values = vec![C64::ZERO; n];
        let mut hi = n;
        let mut iterations = 0;
        while hi > 0 {
            let m = hi - 1;
            if m == 0 {
                values[0] = h[at(0, 0)];
                break;
            }
            let mut lo = m;
            while lo > 0 {
                let s = h[at(lo - 1, lo - 1)].abs() + h[at(lo, lo)].abs();
                if h[at(lo, lo - 1)].abs() <= f64::EPSILON * s.max(f64::MIN_POSITIVE) {
                    h[at(lo, lo - 1)] = C64::ZERO;
                    break;
                }
                lo -= 1;
            }
            if lo == m {
                values[m] = h[at(m, m)];
                hi = m;
                iterations = 0;
                continue;
            }
            iterations += 1;
            // The Wilkinson shift: the eigenvalue of the trailing 2 x 2 block nearer h(m, m),
            // and an exceptional shift every 10 iterations.
            let (a, b, c, d) = (
                h[at(m - 1, m - 1)],
                h[at(m - 1, m)],
                h[at(m, m - 1)],
                h[at(m, m)],
            );
            let tr = (a + d).scale(0.5);
            let disc = ((a - d).scale(0.5) * (a - d).scale(0.5) + b * c).sqrt();
            let (l1, l2) = (tr + disc, tr - disc);
            let mut mu = if (l1 - d).abs() < (l2 - d).abs() {
                l1
            } else {
                l2
            };
            if iterations % 10 == 0 {
                mu = d + C64::new(h[at(m, m - 1)].abs(), 0.0);
            }
            if iterations > 100 * n {
                // No convergence: take the diagonal as it is.
                for k in lo..=m {
                    values[k] = h[at(k, k)];
                }
                hi = lo;
                continue;
            }
            for k in lo..=m {
                h[at(k, k)] -= mu;
            }
            // QR by Givens rotations, then RQ.
            let mut rot: Vec<(C64, C64)> = Vec::with_capacity(m - lo);
            for k in lo..m {
                let (x, y) = (h[at(k, k)], h[at(k + 1, k)]);
                let r = (x.abs().powi(2) + y.abs().powi(2)).sqrt();
                let (cs, sn) = if r == 0.0 {
                    (C64::ONE, C64::ZERO)
                } else {
                    (x.scale(1.0 / r), y.scale(1.0 / r))
                };
                // [cs* sn*; -sn cs] applied to rows k and k + 1.
                for j in k..n {
                    let (p, q) = (h[at(k, j)], h[at(k + 1, j)]);
                    h[at(k, j)] = cs.conj() * p + sn.conj() * q;
                    h[at(k + 1, j)] = cs * q - sn * p;
                }
                rot.push((cs, sn));
            }
            for (r, (cs, sn)) in rot.into_iter().enumerate() {
                let k = lo + r;
                for i in 0..=(k + 1).min(m) {
                    let (p, q) = (h[at(i, k)], h[at(i, k + 1)]);
                    h[at(i, k)] = p * cs + q * sn;
                    h[at(i, k + 1)] = q * cs.conj() - p * sn.conj();
                }
            }
            for k in lo..=m {
                h[at(k, k)] += mu;
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: C64, b: C64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn the_exponential_of_a_rotation_generator_is_a_rotation() {
        // e^[0 -t; t 0] = [cos t, -sin t; sin t, cos t], also for a large t.
        for t in [0.3, 7.0, 40.0] {
            let mut m = Matrix::zeros(2);
            m.set(0, 1, C64::new(-t, 0.0));
            m.set(1, 0, C64::new(t, 0.0));
            let e = m.exp();
            assert!(close(e.at(0, 0), C64::new(t.cos(), 0.0), 1e-11), "{t}");
            assert!(close(e.at(1, 0), C64::new(t.sin(), 0.0), 1e-11), "{t}");
        }
    }

    #[test]
    fn the_eigenvalues_of_a_triangular_and_a_full_matrix_are_found() {
        let mut m = Matrix::zeros(3);
        let v = [C64::new(1.0, 2.0), C64::new(-3.0, 0.5), C64::new(0.0, -1.0)];
        for i in 0..3 {
            m.set(i, i, v[i]);
            for j in i + 1..3 {
                m.set(i, j, C64::new(0.7, -0.2));
            }
        }
        // A similarity S M S^-1 keeps the eigenvalues and fills the matrix.
        let mut s = Matrix::identity(3);
        s.set(1, 0, C64::new(0.4, 0.1));
        s.set(2, 1, C64::new(-0.3, 0.8));
        s.set(0, 2, C64::new(0.2, 0.0));
        let full = s.mul(&m).mul(&s.solve(&Matrix::identity(3)).unwrap());
        let mut e = full.eigenvalues();
        for want in v {
            let k = (0..e.len())
                .min_by(|&a, &b| (e[a] - want).abs().total_cmp(&(e[b] - want).abs()))
                .unwrap();
            assert!(close(e[k], want, 1e-10), "{:?} not in {e:?}", want);
            e.remove(k);
        }
    }

    #[test]
    fn the_lu_solve_inverts_a_matrix() {
        let mut m = Matrix::zeros(3);
        for i in 0..3 {
            for j in 0..3 {
                m.set(
                    i,
                    j,
                    C64::new((i * 3 + j) as f64 * 0.1, (i + 2 * j) as f64 * 0.3 - 0.5),
                );
            }
            m.a[i * 4] += C64::new(2.0, 0.0);
        }
        let inv = m.solve(&Matrix::identity(3)).unwrap();
        let id = m.mul(&inv);
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { C64::ONE } else { C64::ZERO };
                assert!((id.at(i, j) - want).abs() < 1e-12);
            }
        }
    }
}
