//! The numeric real Cholesky factor P A P^T = L L^T of a sparse symmetric positive definite
//! matrix, by the multifrontal method on the supernodes of `symbolic`.
//!
//! Each supernode assembles a dense frontal matrix from the entries of A in its columns and the
//! update matrices of its children (extend-add). A dense partial Cholesky of the front gives the
//! columns of L of the supernode and the update matrix (the Schur complement) for its parent. The
//! postorder of the supernodes makes the update matrices a stack.
//!
//! Sources: I. S. Duff and J. K. Reid, The multifrontal solution of indefinite sparse symmetric
//! linear equations, ACM Trans. Math. Softw. 9 (1983) 302-325; J. W. H. Liu, The multifrontal
//! method for sparse matrix solution: theory and practice, SIAM Review 34 (1992) 82-109;
//! G. H. Golub and C. F. Van Loan, Matrix Computations, 4th ed., 2013, section 4.2 (dense
//! Cholesky).

use crate::sparse::Csr;
use crate::symbolic::Symbolic;

/// A pivot below this fraction of the diagonal of A in its column means that the matrix is
/// singular to the precision of the arithmetic. The last pivot of a singular matrix is about 1e-16
/// of its diagonal, and the smallest pivot of a heat conduction model with a conductivity ratio of
/// 1e4 is far above 1e-10.
const PIVOT: f64 = 1e-10;

/// The factor L, 1 dense block for each supernode.
#[derive(Debug, Clone)]
pub struct Cholesky {
    sym: Symbolic,
    /// The block of supernode `s` starts at `offset[s]`: its rows (m) by its columns (k), column
    /// by column.
    offset: Vec<usize>,
    values: Vec<f64>,
}

/// The update matrix of a supernode for its parent: the rows and the lower triangle, column by
/// column in a full square.
struct Update {
    rows: Vec<u32>,
    values: Vec<f64>,
}

impl Cholesky {
    /// Factor `a` (both triangles, the pattern of the analysis). An error names the column
    /// (old index) whose pivot is not positive.
    pub fn factor(sym: &Symbolic, a: &Csr) -> Result<Cholesky, String> {
        let n = sym.n;
        let ns = sym.supernodes();
        let mut offset = Vec::with_capacity(ns + 1);
        offset.push(0);
        for s in 0..ns {
            let k = sym.start[s + 1] - sym.start[s];
            let m = sym.rows_of(s).len();
            offset.push(offset[s] + k * m);
        }
        let mut values = vec![0.0; offset[ns]];
        let mut pos = vec![usize::MAX; n];
        let mut stack: Vec<Update> = Vec::new();
        for s in 0..ns {
            let rows = sym.rows_of(s);
            let (first, end) = (sym.start[s], sym.start[s + 1]);
            let (m, k) = (rows.len(), end - first);
            for (l, &r) in rows.iter().enumerate() {
                pos[r as usize] = l;
            }
            let mut front = vec![0.0; m * m];
            let mut diag = vec![0.0; k];
            for j in first..end {
                let old = sym.perm[j];
                let col = (j - first) * m;
                for e in a.ptr[old]..a.ptr[old + 1] {
                    let i = sym.iperm[a.idx[e] as usize];
                    if i >= j {
                        front[col + pos[i]] += a.val[e];
                    }
                    if i == j {
                        diag[j - first] = a.val[e];
                    }
                }
            }
            // The children are on the top of the stack.
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
            partial_cholesky(&mut front, m, k, &diag).map_err(|j| {
                let column = sym.perm[first + j];
                format!(
                    "the matrix is not positive definite: the pivot of column {column} is 0 or \
                     less to the precision of the arithmetic (its diagonal is {:e})",
                    diag[j]
                )
            })?;
            values[offset[s]..offset[s + 1]].copy_from_slice(&front[..k * m]);
            if m > k {
                let mu = m - k;
                let mut u = vec![0.0; mu * mu];
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
        Ok(Cholesky {
            sym: sym.clone(),
            offset,
            values,
        })
    }

    /// Solve A x = b. `b` holds the right side and gets the solution.
    pub fn solve(&self, b: &mut [f64]) {
        let sym = &self.sym;
        let mut y: Vec<f64> = sym.perm.iter().map(|&i| b[i]).collect();
        let ns = sym.supernodes();
        for s in 0..ns {
            let rows = sym.rows_of(s);
            let (first, end) = (sym.start[s], sym.start[s + 1]);
            let m = rows.len();
            let block = &self.values[self.offset[s]..self.offset[s + 1]];
            for j in 0..end - first {
                let col = &block[j * m..(j + 1) * m];
                let v = y[first + j] / col[j];
                y[first + j] = v;
                for r in j + 1..m {
                    y[rows[r] as usize] -= col[r] * v;
                }
            }
        }
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
                y[first + j] = v / col[j];
            }
        }
        for (k, &i) in sym.perm.iter().enumerate() {
            b[i] = y[k];
        }
    }

    /// The number of stored values of L: the full diagonal block of each supernode and its
    /// rows below.
    pub fn entries(&self) -> usize {
        self.values.len()
    }
}

/// The partial Cholesky factor of the first `k` columns of the dense `m` x `m` matrix `f` (lower
/// triangle, column by column). The columns 0..k get L, and the lower triangle of columns k..m
/// gets the Schur complement. An error is the local column with a pivot that is not positive:
/// below `PIVOT` times its diagonal `diag` in A.
fn partial_cholesky(f: &mut [f64], m: usize, k: usize, diag: &[f64]) -> Result<(), usize> {
    for j in 0..k {
        let (done, rest) = f.split_at_mut((j + 1) * m);
        let col = &mut done[j * m..];
        let d = col[j];
        if !(d > PIVOT * diag[j].abs()) {
            return Err(j);
        }
        let l = d.sqrt();
        col[j] = l;
        let inv = 1.0 / l;
        for v in &mut col[j + 1..m] {
            *v *= inv;
        }
        // The other columns of the supernode get the update of column j.
        let col = &done[j * m..];
        for c in j + 1..k {
            let target = &mut rest[(c - j - 1) * m..(c - j) * m];
            let factor = col[c];
            for r in c..m {
                target[r] -= col[r] * factor;
            }
        }
    }
    // The Schur complement: F22 -= L21 L21^T, 4 columns of L at a time.
    let (l, f22) = f.split_at_mut(k * m);
    for c in k..m {
        let target = &mut f22[(c - k) * m..(c - k + 1) * m];
        let mut p = 0;
        while p + 4 <= k {
            let (c0, c1, c2, c3) = (
                &l[p * m..(p + 1) * m],
                &l[(p + 1) * m..(p + 2) * m],
                &l[(p + 2) * m..(p + 3) * m],
                &l[(p + 3) * m..(p + 4) * m],
            );
            let (f0, f1, f2, f3) = (c0[c], c1[c], c2[c], c3[c]);
            for r in c..m {
                target[r] -= c0[r] * f0 + c1[r] * f1 + c2[r] * f2 + c3[r] * f3;
            }
            p += 4;
        }
        while p < k {
            let c0 = &l[p * m..(p + 1) * m];
            let f0 = c0[c];
            for r in c..m {
                target[r] -= c0[r] * f0;
            }
            p += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::nested_dissection;

    /// The 5-point Laplacian on an n x n grid with a positive shift, so that it is positive
    /// definite.
    pub(crate) fn laplacian(n: usize) -> (Csr, Vec<[f64; 3]>) {
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
        let mut a = Csr::pattern(n * n, elements.iter().map(|e| &e[..]));
        for e in &elements {
            a.add_element(e, &[1.0, -1.0, -1.0, 1.0]);
        }
        for i in 0..n * n {
            let k = a.find(i, i).unwrap();
            a.val[k] += 0.01;
        }
        let xyz = (0..n * n)
            .map(|k| [(k % n) as f64, (k / n) as f64, 0.0])
            .collect();
        (a, xyz)
    }

    #[test]
    fn the_factor_solves_a_grid_laplacian_to_the_precision_of_the_arithmetic() {
        let (a, xyz) = laplacian(40);
        let order = nested_dissection(&a, &xyz);
        let sym = Symbolic::analyse(&a, &order);
        let chol = Cholesky::factor(&sym, &a).unwrap();
        let x: Vec<f64> = (0..a.n).map(|i| (i as f64 * 0.37).sin()).collect();
        let mut b = vec![0.0; a.n];
        a.mul(&x, &mut b);
        chol.solve(&mut b);
        let err = x
            .iter()
            .zip(&b)
            .map(|(p, q)| (p - q).abs())
            .fold(0.0, f64::max);
        assert!(err < 1e-9, "error {err}");
        // Nested dissection keeps the factor small: about n log n entries in 2D.
        assert!(sym.entries() < 40_000, "{} entries", sym.entries());
    }

    #[test]
    fn a_singular_matrix_is_an_error() {
        let (mut a, xyz) = laplacian(10);
        for i in 0..a.n {
            let k = a.find(i, i).unwrap();
            a.val[k] -= 0.01;
        }
        let sym = Symbolic::analyse(&a, &nested_dissection(&a, &xyz));
        let e = Cholesky::factor(&sym, &a).unwrap_err();
        assert!(
            e.contains("not positive definite") || e.contains("singular"),
            "{e}"
        );
    }
}
