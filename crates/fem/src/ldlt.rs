//! The numeric complex symmetric factor P A P^T = L D L^T of a sparse complex symmetric matrix
//! (A = A^T, not Hermitian), by the multifrontal method on the supernodes of `symbolic`: the
//! same ordering and symbolic analysis as the real Cholesky factor of `cholesky`.
//!
//! A is the pair of 2 real matrices of 1 pattern: A = re + j im. L has a unit diagonal; the
//! diagonal of the stored block holds D. There are no pivot swaps (static pivots in the order of
//! the analysis). The matrices of the quasi-static field problems of `line` are the sum of a real
//! positive definite part and j times a positive semidefinite part, and their pivots stay away
//! from 0 with no swap.
//!
//! Sources: I. S. Duff and J. K. Reid, The multifrontal solution of indefinite sparse symmetric
//! linear equations, ACM Trans. Math. Softw. 9 (1983) 302-325; J. W. H. Liu, The multifrontal
//! method for sparse matrix solution: theory and practice, SIAM Review 34 (1992) 82-109;
//! G. H. Golub and C. F. Van Loan, Matrix Computations, 4th ed., 2013, section 4.1 (the LDL^T
//! factor of a symmetric matrix, which holds for a complex symmetric matrix with the transpose in
//! place of the conjugate transpose); A. Bunse-Gerstner and R. Stöver, On a conjugate
//! gradient-type method for solving complex symmetric linear systems, Linear Algebra Appl. 287
//! (1999) 105-123 (complex symmetric systems of the eddy current equations).

use crate::complex::C64;
use crate::sparse::Csr;
use crate::symbolic::Symbolic;

/// A pivot below this fraction of the diagonal of A in its column means that the matrix is
/// singular to the precision of the arithmetic, as in `cholesky`.
const PIVOT: f64 = 1e-10;

/// The factor L D L^T, 1 dense block for each supernode.
#[derive(Debug, Clone)]
pub struct Ldlt {
    sym: Symbolic,
    offset: Vec<usize>,
    values: Vec<C64>,
}

struct Update {
    rows: Vec<u32>,
    values: Vec<C64>,
}

impl Ldlt {
    /// Factor A = `re` + j `im`: 2 matrices with the pattern of the analysis (both triangles).
    /// An error names the column (old index) whose pivot is 0.
    pub fn factor(sym: &Symbolic, re: &Csr, im: &Csr) -> Result<Ldlt, String> {
        assert!(
            re.ptr == im.ptr && re.idx == im.idx,
            "the real and the imaginary part have 1 pattern"
        );
        let n = sym.n;
        let ns = sym.supernodes();
        let mut offset = Vec::with_capacity(ns + 1);
        offset.push(0);
        for s in 0..ns {
            let k = sym.start[s + 1] - sym.start[s];
            let m = sym.rows_of(s).len();
            offset.push(offset[s] + k * m);
        }
        let mut values = vec![C64::ZERO; offset[ns]];
        let mut pos = vec![usize::MAX; n];
        let mut stack: Vec<Update> = Vec::new();
        for s in 0..ns {
            let rows = sym.rows_of(s);
            let (first, end) = (sym.start[s], sym.start[s + 1]);
            let (m, k) = (rows.len(), end - first);
            for (l, &r) in rows.iter().enumerate() {
                pos[r as usize] = l;
            }
            let mut front = vec![C64::ZERO; m * m];
            let mut diag = vec![0.0; k];
            for j in first..end {
                let old = sym.perm[j];
                let col = (j - first) * m;
                for e in re.ptr[old]..re.ptr[old + 1] {
                    let i = sym.iperm[re.idx[e] as usize];
                    let v = C64::new(re.val[e], im.val[e]);
                    if i >= j {
                        front[col + pos[i]] += v;
                    }
                    if i == j {
                        diag[j - first] = v.abs();
                    }
                }
            }
            for _ in 0..sym.children[s] {
                let u = stack.pop().expect("each child pushed its update");
                let mu = u.rows.len();
                let at: Vec<usize> = u.rows.iter().map(|&r| pos[r as usize]).collect();
                for b in 0..mu {
                    let col = at[b] * m;
                    let src = &u.values[b * mu..(b + 1) * mu];
                    for a in b..mu {
                        front[col + at[a]] += src[a];
                    }
                }
            }
            partial_ldlt(&mut front, m, k, &diag).map_err(|j| {
                let column = sym.perm[first + j];
                format!(
                    "the matrix is singular: the pivot of column {column} is 0 to the precision \
                     of the arithmetic (its diagonal is {:e})",
                    diag[j]
                )
            })?;
            values[offset[s]..offset[s + 1]].copy_from_slice(&front[..k * m]);
            if m > k {
                let mu = m - k;
                let mut u = vec![C64::ZERO; mu * mu];
                for b in 0..mu {
                    let src = (k + b) * m + k;
                    u[b * mu + b..(b + 1) * mu].copy_from_slice(&front[src + b..src + mu]);
                }
                stack.push(Update {
                    rows: rows[k..].to_vec(),
                    values: u,
                });
            }
            for &r in rows {
                pos[r as usize] = usize::MAX;
            }
        }
        Ok(Ldlt {
            sym: sym.clone(),
            offset,
            values,
        })
    }

    /// Solve A x = b. `b` holds the right side and gets the solution.
    pub fn solve(&self, b: &mut [C64]) {
        let sym = &self.sym;
        let mut y: Vec<C64> = sym.perm.iter().map(|&i| b[i]).collect();
        let ns = sym.supernodes();
        // L y = b, then D.
        for s in 0..ns {
            let rows = sym.rows_of(s);
            let (first, end) = (sym.start[s], sym.start[s + 1]);
            let m = rows.len();
            let block = &self.values[self.offset[s]..self.offset[s + 1]];
            for j in 0..end - first {
                let col = &block[j * m..(j + 1) * m];
                let v = y[first + j];
                for r in j + 1..m {
                    y[rows[r] as usize] -= col[r] * v;
                }
            }
        }
        for s in 0..ns {
            let m = sym.rows_of(s).len();
            let block = &self.values[self.offset[s]..self.offset[s + 1]];
            for j in 0..sym.start[s + 1] - sym.start[s] {
                y[sym.start[s] + j] = y[sym.start[s] + j] / block[j * m + j];
            }
        }
        // L^T x = y.
        for s in (0..ns).rev() {
            let rows = sym.rows_of(s);
            let (first, end) = (sym.start[s], sym.start[s + 1]);
            let m = rows.len();
            let block = &self.values[self.offset[s]..self.offset[s + 1]];
            for j in (0..end - first).rev() {
                let col = &block[j * m..(j + 1) * m];
                let mut v = y[first + j];
                for r in j + 1..m {
                    v -= col[r] * y[rows[r] as usize];
                }
                y[first + j] = v;
            }
        }
        for (k, &i) in sym.perm.iter().enumerate() {
            b[i] = y[k];
        }
    }
}

/// The partial L D L^T factor of the first `k` columns of the dense `m` x `m` complex symmetric
/// matrix `f` (lower triangle, column by column). Column j gets D(j) on the diagonal and L below
/// it; the lower triangle of columns k..m gets the Schur complement. An error is the local column
/// whose pivot is below `PIVOT` times the absolute value of its diagonal `diag` in A.
fn partial_ldlt(f: &mut [C64], m: usize, k: usize, diag: &[f64]) -> Result<(), usize> {
    // The column j of L times D(j), for the updates.
    let mut ld = vec![C64::ZERO; m];
    for j in 0..k {
        let (done, rest) = f.split_at_mut((j + 1) * m);
        let col = &mut done[j * m..];
        let d = col[j];
        if !(d.abs() > PIVOT * diag[j]) {
            return Err(j);
        }
        ld[j + 1..m].copy_from_slice(&col[j + 1..m]);
        let inv = C64::ONE / d;
        for v in &mut col[j + 1..m] {
            *v *= inv;
        }
        let col = &done[j * m..];
        for c in j + 1..k {
            let target = &mut rest[(c - j - 1) * m..(c - j) * m];
            let factor = ld[c];
            for r in c..m {
                target[r] -= col[r] * factor;
            }
        }
        // The Schur complement gets the update of column j.
        for c in k..m {
            let target = &mut rest[(c - j - 1) * m..(c - j) * m];
            let factor = ld[c];
            if factor == C64::ZERO {
                continue;
            }
            for r in c..m {
                target[r] -= col[r] * factor;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::nested_dissection;

    /// A 2D grid Laplacian plus j times a mass term on half of the nodes: the matrix of an eddy
    /// current problem.
    #[test]
    fn the_factor_solves_a_complex_symmetric_grid_matrix() {
        let n = 30;
        let id = |i: usize, j: usize| (i + n * j) as u32;
        let mut elements = Vec::new();
        for j in 0..n {
            for i in 0..n {
                if i + 1 < n {
                    elements.push([id(i, j), id(i + 1, j)]);
                }
                if j + 1 < n {
                    elements.push([id(i, j), id(i, j + 1)]);
                }
            }
        }
        let mut re = Csr::pattern(n * n, elements.iter().map(|e| &e[..]));
        for e in &elements {
            re.add_element(e, &[1.0, -1.0, -1.0, 1.0]);
        }
        let mut im = re.clone();
        im.val.fill(0.0);
        for i in 0..n * n {
            let k = re.find(i, i).unwrap();
            re.val[k] += 1e-3;
            if i % n < n / 2 {
                im.val[k] += 0.7;
            }
        }
        let xyz: Vec<[f64; 3]> = (0..n * n)
            .map(|k| [(k % n) as f64, (k / n) as f64, 0.0])
            .collect();
        let sym = Symbolic::analyse(&re, &nested_dissection(&re, &xyz));
        let f = Ldlt::factor(&sym, &re, &im).unwrap();
        let x: Vec<C64> = (0..n * n)
            .map(|i| C64::new((i as f64 * 0.37).sin(), (i as f64 * 0.11).cos()))
            .collect();
        let mut b = vec![C64::ZERO; n * n];
        for i in 0..n * n {
            for e in re.ptr[i]..re.ptr[i + 1] {
                b[i] += C64::new(re.val[e], im.val[e]) * x[re.idx[e] as usize];
            }
        }
        f.solve(&mut b);
        let err = x
            .iter()
            .zip(&b)
            .map(|(p, q)| (*p - *q).abs())
            .fold(0.0, f64::max);
        assert!(err < 1e-9, "error {err}");
    }
}
