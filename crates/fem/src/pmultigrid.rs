//! A 2-level p-multigrid preconditioner for the quadratic tetrahedra (`tet10`): the coarse level
//! is the linear tetrahedra on the same mesh.
//!
//! The linear field of the corners is a quadratic field with each edge point at the mean of
//! its 2 corners. Thus the prolongation P puts each corner unknown on itself and half of it on
//! each edge point of its edges, and the linear space is a subspace of the quadratic space. The
//! coarse matrix is the Galerkin product P^T A P, the stiffness of the linear tetrahedra, and
//! the sparse Cholesky factor solves it. The linear field holds the rigid-body modes and each
//! smooth field, so the error that is left after the coarse correction varies inside the
//! cells, and Gauss-Seidel on the quadratic matrix removes it. A fixed corner unknown has no
//! coarse unknown: its edge points take half of the other corner only.
//!
//! The V-cycle is forward Gauss-Seidel, the coarse correction, then backward Gauss-Seidel, so it
//! is a symmetric positive definite preconditioner of conjugate gradients.
//!
//! Sources: E. M. Rønquist and A. T. Patera, Spectral element multigrid. I. Formulation and
//! numerical results, J. Sci. Comput. 2 (1987) 389-406 (multigrid in the polynomial degree);
//! B. T. Helenbrook, D. Mavriplis and H. L. Atkins, Analysis of p-multigrid for continuous and
//! discontinuous finite element discretizations, AIAA paper 2003-3989 (the p = 2 to p = 1
//! level); W. L. Briggs, V. E. Henson and S. F. McCormick, A Multigrid Tutorial, 2nd ed., SIAM
//! 2000 (the 2-grid cycle and the Galerkin coarse matrix); Y. Saad, Iterative Methods for
//! Sparse Linear Systems, 2nd ed., SIAM 2003, sections 4.1 (Gauss-Seidel, the symmetric sweep)
//! and 9.2 (a symmetric preconditioner of conjugate gradients).

use crate::cholesky::Cholesky;
use crate::order::nested_dissection;
use crate::sparse::{Csr, Rect};
use crate::symbolic::Symbolic;

/// The Gauss-Seidel sweeps before and after the coarse correction. Reason: in the test
/// `crossover`, 2 sweeps against 1 took 3.7 s against 5.1 s on a ring of 86,237 unknowns, and
/// 14.7 s against 42.0 s on a plate of 305,715.
const SWEEPS: usize = 2;

pub struct PMultigrid {
    p: Rect,
    diag: Vec<f64>,
    coarse: Cholesky,
    /// The unknowns of the coarse level.
    pub coarse_unknowns: usize,
}

impl PMultigrid {
    /// The preconditioner of `a` with the prolongation `p` from the coarse unknowns at the
    /// points `coarse`.
    pub fn new(a: &Csr, p: Rect, coarse: &[[f64; 3]]) -> Result<PMultigrid, String> {
        let ac = p.galerkin(a);
        let sym = Symbolic::analyse(&ac, &nested_dissection(&ac, coarse));
        let factor = Cholesky::factor(&sym, &ac)?;
        let diag = a.diagonal();
        if diag.iter().any(|d| !(*d > 0.0)) {
            return Err("the matrix has a diagonal entry that is not above 0".into());
        }
        Ok(PMultigrid {
            coarse_unknowns: p.cols,
            p,
            diag,
            coarse: factor,
        })
    }

    /// z = M^-1 r for the matrix `a` of `new`.
    pub fn apply(&self, a: &Csr, r: &[f64], z: &mut [f64]) {
        let n = a.n;
        z.fill(0.0);
        let row = |z: &[f64], i: usize| -> f64 {
            let mut s = r[i];
            for k in a.ptr[i]..a.ptr[i + 1] {
                let j = a.idx[k] as usize;
                if j != i {
                    s -= a.val[k] * z[j];
                }
            }
            s / self.diag[i]
        };
        for _ in 0..SWEEPS {
            for i in 0..n {
                let v = row(z, i);
                z[i] = v;
            }
        }
        let mut az = vec![0.0; n];
        a.mul(z, &mut az);
        let res: Vec<f64> = r.iter().zip(&az).map(|(r, a)| r - a).collect();
        let mut rc = vec![0.0; self.p.cols];
        self.p.mul_transpose(&res, &mut rc);
        self.coarse.solve(&mut rc);
        self.p.mul(&rc, &mut az);
        for i in 0..n {
            z[i] += az[i];
        }
        for _ in 0..SWEEPS {
            for i in (0..n).rev() {
                let v = row(z, i);
                z[i] = v;
            }
        }
    }
}

/// The prolongation of the free unknowns of a quadratic mesh, and the point of each coarse
/// unknown. Unknown `3 v + c` is component c of point v. `free` lists the free unknowns,
/// increasing. `corners` and `edges` are those of `tet10::Quadratic`.
pub fn prolongation(
    points: &[[f64; 3]],
    corners: usize,
    edges: &[[u32; 2]],
    free: &[usize],
) -> (Rect, Vec<[f64; 3]>) {
    // The coarse unknown of each free corner unknown.
    let mut coarse = vec![u32::MAX; 3 * corners];
    let mut xyz = Vec::new();
    for &i in free {
        if i < 3 * corners {
            coarse[i] = xyz.len() as u32;
            xyz.push(points[i / 3]);
        }
    }
    let mut ptr = Vec::with_capacity(free.len() + 1);
    ptr.push(0);
    let (mut idx, mut val) = (Vec::new(), Vec::new());
    for &i in free {
        let (v, c) = (i / 3, i % 3);
        if v < corners {
            idx.push(coarse[i]);
            val.push(1.0);
        } else {
            let mut ends: Vec<u32> = edges[v - corners]
                .iter()
                .map(|&a| coarse[3 * a as usize + c])
                .filter(|&k| k != u32::MAX)
                .collect();
            ends.sort_unstable();
            for k in ends {
                idx.push(k);
                val.push(0.5);
            }
        }
        ptr.push(idx.len());
    }
    (
        Rect {
            rows: free.len(),
            cols: xyz.len(),
            ptr,
            idx,
            val,
        },
        xyz,
    )
}
