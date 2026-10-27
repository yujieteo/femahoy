//! Conjugate gradients with a geometric multigrid preconditioner on a rectilinear grid.
//!
//! Each coarser level keeps every other grid plane of each axis (and the last plane). The
//! prolongation P is a tensor product of 1D interpolations from the coarse grid points to the
//! fine ones. The weights of a point between 2 coarse planes come from the matrix: its couplings
//! to the 2 planes, summed over the other axes (the collapsed stencil). Thus the interpolation
//! follows a jump of the conductivity, for example at a thin layer of glue between 2 metals.
//! The geometric weights of the graded planes are the fallback. The coarse matrix is the Galerkin product
//! P^T A P, so it needs no coarse model and holds the jumps of the material. The smoother is
//! line Gauss-Seidel along x, then y, then z before the coarse correction, and backward along z,
//! y, then x after it, so the V-cycle is a symmetric positive definite preconditioner. The lines
//! along each axis make it robust for thin parts in each direction, for example the skin and
//! the ribs of a stiffened panel. The coarsest level is solved with the sparse
//! Cholesky factor.
//!
//! Sources: W. L. Briggs, V. E. Henson and S. F. McCormick, A Multigrid Tutorial, 2nd ed., SIAM
//! 2000 (V-cycle, interpolation, Galerkin coarse operator); R. E. Alcouffe, A. Brandt, J. E.
//! Dendy and J. W. Painter, The multi-grid method for the diffusion equation with strongly
//! discontinuous coefficients, SIAM J. Sci. Stat. Comput. 2 (1981) 430-454 (operator-dependent
//! interpolation); U. Trottenberg, C. W. Oosterlee and
//! A. Schüller, Multigrid, Academic Press 2001, section 5.1 (anisotropy and line smoothers) and
//! section 7.7 (multigrid as a preconditioner of
//! conjugate gradients); M. R. Hestenes and E. Stiefel, Methods of conjugate gradients for
//! solving linear systems, J. Res. Nat. Bur. Standards 49 (1952) 409-436; Y. Saad, Iterative
//! Methods for Sparse Linear Systems, 2nd ed., SIAM 2003, algorithm 9.1 (preconditioned CG).

use crate::cholesky::Cholesky;
use crate::order::nested_dissection;
use crate::sparse::{Csr, Rect};
use crate::symbolic::Symbolic;

/// A level with this many unknowns or less is solved directly.
const COARSEST: usize = 3000;

/// The unknowns of a grid level: the planes of each axis and the grid point of each unknown.
#[derive(Debug, Clone)]
pub struct Level {
    pub planes: [Vec<f64>; 3],
    pub points: Vec<[u32; 3]>,
}

/// The hierarchy of the matrices and prolongations, and the factor of the coarsest matrix.
pub struct Multigrid {
    a: Vec<Csr>,
    p: Vec<Rect>,
    /// The lines of the smoother along each axis, of each level but the coarsest.
    lines: Vec<[Vec<Vec<u32>>; 3]>,
    coarse: Cholesky,
}

impl Multigrid {
    /// The hierarchy of the matrix `a` whose unknown `k` is the grid point `level.points[k]`.
    pub fn new(a: Csr, level: Level) -> Result<Multigrid, String> {
        let mut mats = vec![a];
        let mut ps = Vec::new();
        let mut lines = Vec::new();
        let mut level = level;
        loop {
            let a = mats.last().expect("1 matrix");
            if a.n <= COARSEST || ps.len() >= 30 {
                break;
            }
            let Some((p, coarse)) = coarsen(&level, a) else {
                break;
            };
            if coarse.points.len() * 10 > a.n * 9 {
                break;
            }
            lines.push(std::array::from_fn(|d| lines_of(&level, d)));
            mats.push(p.galerkin(a));
            ps.push(p);
            level = coarse;
        }
        let a = mats.last().expect("1 matrix");
        let xyz: Vec<[f64; 3]> = level
            .points
            .iter()
            .map(|p| std::array::from_fn(|d| level.planes[d][p[d] as usize]))
            .collect();
        let sym = Symbolic::analyse(a, &nested_dissection(a, &xyz));
        let coarse = Cholesky::factor(&sym, a)?;
        Ok(Multigrid {
            a: mats,
            p: ps,
            lines,
            coarse,
        })
    }

    /// The number of levels.
    pub fn levels(&self) -> usize {
        self.a.len()
    }

    /// z = M^-1 r: 1 V-cycle from z = 0.
    pub fn apply(&self, r: &[f64], z: &mut [f64]) {
        self.cycle(0, r, z);
    }

    fn cycle(&self, l: usize, b: &[f64], x: &mut [f64]) {
        if l + 1 == self.a.len() {
            x.copy_from_slice(b);
            self.coarse.solve(x);
            return;
        }
        let a = &self.a[l];
        let p = &self.p[l];
        x.fill(0.0);
        for lines in &self.lines[l] {
            line_gauss_seidel(a, lines, b, x, true);
        }
        let mut r = vec![0.0; a.n];
        a.mul(x, &mut r);
        for (ri, bi) in r.iter_mut().zip(b) {
            *ri = bi - *ri;
        }
        let mut rc = vec![0.0; p.cols];
        p.mul_transpose(&r, &mut rc);
        let mut ec = vec![0.0; p.cols];
        self.cycle(l + 1, &rc, &mut ec);
        p.mul(&ec, &mut r);
        for (xi, ei) in x.iter_mut().zip(&r) {
            *xi += ei;
        }
        for lines in self.lines[l].iter().rev() {
            line_gauss_seidel(a, lines, b, x, false);
        }
    }
}

/// The lines of unknowns along `axis`: the unknowns of 1 row of grid points along the axis, in
/// order, split where a point has no unknown. A thin part couples its points across its
/// thickness much more than along it, and a line solve across the thickness smooths the error
/// that a point smoother leaves.
fn lines_of(level: &Level, axis: usize) -> Vec<Vec<u32>> {
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut order: Vec<u32> = (0..level.points.len() as u32).collect();
    let key = |k: &u32| {
        let p = level.points[*k as usize];
        (p[v], p[u], p[axis])
    };
    order.sort_unstable_by_key(key);
    let mut lines: Vec<Vec<u32>> = Vec::new();
    let mut last: Option<[u32; 3]> = None;
    for k in order {
        let p = level.points[k as usize];
        let next = last.is_some_and(|q| q[u] == p[u] && q[v] == p[v] && q[axis] + 1 == p[axis]);
        if next {
            lines.last_mut().expect("a line").push(k);
        } else {
            lines.push(vec![k]);
        }
        last = Some(p);
    }
    lines
}

/// 1 line Gauss-Seidel sweep of A x = b, forward or backward: each line solves its tridiagonal
/// system (the couplings of each unknown to its 2 neighbours on the line) with the other
/// unknowns at their last values.
fn line_gauss_seidel(a: &Csr, lines: &[Vec<u32>], b: &[f64], x: &mut [f64], forward: bool) {
    let mut lower = Vec::new();
    let mut diag = Vec::new();
    let mut upper = Vec::new();
    let mut rhs = Vec::new();
    let mut solve = |line: &[u32], x: &mut [f64]| {
        let m = line.len();
        lower.clear();
        diag.clear();
        upper.clear();
        rhs.clear();
        for (t, &i) in line.iter().enumerate() {
            let i = i as usize;
            let prev = if t > 0 { line[t - 1] } else { u32::MAX };
            let next = if t + 1 < m { line[t + 1] } else { u32::MAX };
            let (mut l, mut d, mut u, mut s) = (0.0, 0.0, 0.0, b[i]);
            for k in a.ptr[i]..a.ptr[i + 1] {
                let j = a.idx[k];
                if j as usize == i {
                    d = a.val[k];
                } else if j == prev {
                    l = a.val[k];
                } else if j == next {
                    u = a.val[k];
                } else {
                    s -= a.val[k] * x[j as usize];
                }
            }
            lower.push(l);
            diag.push(d);
            upper.push(u);
            rhs.push(s);
        }
        // The Thomas algorithm.
        for t in 1..m {
            let f = lower[t] / diag[t - 1];
            diag[t] -= f * upper[t - 1];
            rhs[t] -= f * rhs[t - 1];
        }
        let mut v = rhs[m - 1] / diag[m - 1];
        x[line[m - 1] as usize] = v;
        for t in (0..m - 1).rev() {
            v = (rhs[t] - upper[t] * v) / diag[t];
            x[line[t] as usize] = v;
        }
    };
    if forward {
        lines.iter().for_each(|line| solve(line, x));
    } else {
        lines.iter().rev().for_each(|line| solve(line, x));
    }
}

/// The next coarser level and the prolongation to `fine`. `None` when no axis has more than 2
/// planes.
fn coarsen(fine: &Level, a: &Csr) -> Option<(Rect, Level)> {
    // For each fine plane: the coarse planes and their weights.
    let mut map: [Vec<[(u32, f64); 2]>; 3] = Default::default();
    let mut planes: [Vec<f64>; 3] = Default::default();
    let mut any = false;
    for d in 0..3 {
        let p = &fine.planes[d];
        let n = p.len();
        let keep: Vec<usize> = if n <= 2 {
            (0..n).collect()
        } else {
            any = true;
            let mut k: Vec<usize> = (0..n).step_by(2).collect();
            if *k.last().expect("1 plane") != n - 1 {
                k.push(n - 1);
            }
            k
        };
        planes[d] = keep.iter().map(|&i| p[i]).collect();
        let mut c = 0;
        map[d] = (0..n)
            .map(|i| {
                while keep[c] < i {
                    c += 1;
                }
                if keep[c] == i {
                    [(c as u32, 1.0), (c as u32, 0.0)]
                } else {
                    let (lo, hi) = (keep[c - 1], keep[c]);
                    let w = (p[i] - p[lo]) / (p[hi] - p[lo]);
                    [((c - 1) as u32, 1.0 - w), (c as u32, w)]
                }
            })
            .collect();
    }
    if !any {
        return None;
    }
    let dims = [planes[0].len(), planes[1].len(), planes[2].len()];
    let flat = |q: [u32; 3]| q[0] as usize + dims[0] * (q[1] as usize + dims[1] * q[2] as usize);
    let mut number = vec![u32::MAX; dims[0] * dims[1] * dims[2]];
    let mut points = Vec::new();
    let mut ptr = vec![0];
    let (mut idx, mut val) = (Vec::new(), Vec::new());
    for (row, f) in fine.points.iter().enumerate() {
        let start = idx.len();
        let weights: [[(u32, f64); 2]; 3] = std::array::from_fn(|d| {
            let mut w = map[d][f[d] as usize];
            if w[0].0 != w[1].0 {
                // The operator weights of a point between 2 coarse planes: the couplings of its
                // row to the plane below and to the plane above, summed over the other axes.
                let (mut lo, mut hi) = (0.0, 0.0);
                for k in a.ptr[row]..a.ptr[row + 1] {
                    let g = fine.points[a.idx[k] as usize];
                    if g[d] + 1 == f[d] {
                        lo -= a.val[k];
                    } else if g[d] == f[d] + 1 {
                        hi -= a.val[k];
                    }
                }
                if lo > 0.0 && hi > 0.0 {
                    w[0].1 = lo / (lo + hi);
                    w[1].1 = hi / (lo + hi);
                }
            }
            w
        });
        for &(cx, wx) in &weights[0] {
            for &(cy, wy) in &weights[1] {
                for &(cz, wz) in &weights[2] {
                    let w = wx * wy * wz;
                    if w == 0.0 {
                        continue;
                    }
                    let q = [cx, cy, cz];
                    let slot = &mut number[flat(q)];
                    if *slot == u32::MAX {
                        *slot = points.len() as u32;
                        points.push(q);
                    }
                    idx.push(*slot);
                    val.push(w);
                }
            }
        }
        let mut pairs: Vec<(u32, f64)> = idx[start..]
            .iter()
            .copied()
            .zip(val[start..].iter().copied())
            .collect();
        pairs.sort_unstable_by_key(|p| p.0);
        for (k, (j, v)) in pairs.into_iter().enumerate() {
            idx[start + k] = j;
            val[start + k] = v;
        }
        ptr.push(idx.len());
    }
    let p = Rect {
        rows: fine.points.len(),
        cols: points.len(),
        ptr,
        idx,
        val,
    };
    Some((p, Level { planes, points }))
}

/// The end of a PCG solve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Convergence {
    pub iterations: usize,
    /// |b - A x| / |b| at the end.
    pub residual: f64,
}

/// Solve A x = b by conjugate gradients with the preconditioner `precondition` (z = M^-1 r),
/// from the start value in `x`, to |b - A x| <= `tol` |b|. `cancelled` stops the solve. An error
/// tells that the solve did not reach the tolerance in `max_iter` iterations, or was cancelled.
pub fn pcg(
    a: &Csr,
    b: &[f64],
    x: &mut [f64],
    precondition: &dyn Fn(&[f64], &mut [f64]),
    tol: f64,
    max_iter: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Convergence, String> {
    let n = a.n;
    let dot = |u: &[f64], v: &[f64]| u.iter().zip(v).map(|(p, q)| p * q).sum::<f64>();
    let norm_b = dot(b, b).sqrt();
    if norm_b == 0.0 {
        x.fill(0.0);
        return Ok(Convergence {
            iterations: 0,
            residual: 0.0,
        });
    }
    let mut r = vec![0.0; n];
    a.mul(x, &mut r);
    for (ri, bi) in r.iter_mut().zip(b) {
        *ri = bi - *ri;
    }
    let mut z = vec![0.0; n];
    precondition(&r, &mut z);
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    let mut q = vec![0.0; n];
    for it in 0..=max_iter {
        let residual = dot(&r, &r).sqrt() / norm_b;
        if residual <= tol {
            return Ok(Convergence {
                iterations: it,
                residual,
            });
        }
        if it == max_iter {
            return Err(format!(
                "conjugate gradients did not converge: the residual is {residual:.2e} after \
                 {max_iter} iterations"
            ));
        }
        if cancelled() {
            return Err("cancelled".into());
        }
        a.mul(&p, &mut q);
        let alpha = rz / dot(&p, &q);
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * q[i];
        }
        precondition(&r, &mut z);
        let rz_new = dot(&r, &z);
        let beta = rz_new / rz;
        rz = rz_new;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
    }
    unreachable!("the loop returns")
}
