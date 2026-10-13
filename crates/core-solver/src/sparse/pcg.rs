//! Conjugate gradients with a Jacobi preconditioner, on a team of threads.
//!
//! The result does not depend on the number of threads. The rows are cut into fixed blocks
//! of [`BLOCK`] rows, and a thread owns a run of whole blocks. Each row sum of the matrix
//! product has one owner and runs in column order. Each dot product is a sum for each
//! block, and every thread adds the block sums in block order. Thus a run with 1 thread
//! and a run with 10 threads give the same bits. The target machine has 4 to 6 cores, and
//! the large models use this solver. Decision D-06, see DESIGN.md.
//!
//! The matrix keeps only its lower triangle in column form. A column of it is also a row of
//! the upper triangle. For the strict lower part of each row, the solver keeps a row copy
//! that it makes again at each `factor`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Barrier;

use super::{not_spd, LinearSolver, Matrix};
use crate::SolverError;

/// Rows in one block. The block sums of a dot product do not depend on the thread count.
const BLOCK: usize = 1024;

/// Iterations between two calls of the progress callback.
const PROGRESS_EVERY: usize = 25;

/// Conjugate gradients with a Jacobi preconditioner.
pub struct PcgSolver {
    pub tolerance: f64,
    pub max_iterations: usize,
    /// Threads of the solve. The result is the same for every value.
    pub threads: usize,
    /// The strict lower triangle in row form: row `i` is `ptr[i]..ptr[i + 1]`.
    ptr: Vec<u32>,
    col: Vec<u32>,
    val: Vec<f64>,
    inv_diag: Vec<f64>,
}

impl PcgSolver {
    pub fn new(tolerance: f64, max_iterations: usize, threads: usize) -> Self {
        PcgSolver {
            tolerance,
            max_iterations,
            threads: threads.max(1),
            ptr: Vec::new(),
            col: Vec::new(),
            val: Vec::new(),
            inv_diag: Vec::new(),
        }
    }

    /// Bytes of the row copy and the preconditioner for a matrix of `n` rows and `nnz`
    /// lower entries.
    pub fn bytes(n: usize, nnz: usize) -> usize {
        12 * nnz.saturating_sub(n) + 4 * (n + 1) + 8 * n
    }

    /// Row `i` of `a` times `x`: the strict lower part from the row copy, then the column
    /// `i` of `a`, which is the diagonal and the upper part of the row.
    fn row(&self, a: &Matrix, i: usize, x: &[AtomicU64]) -> f64 {
        let get = |j: u32| f64::from_bits(x[j as usize].load(Ordering::Relaxed));
        let mut acc = 0.0;
        for p in self.ptr[i] as usize..self.ptr[i + 1] as usize {
            acc += self.val[p] * get(self.col[p]);
        }
        for p in a.col_ptr[i] as usize..a.col_ptr[i + 1] as usize {
            acc += a.values[p] * get(a.row_idx[p]);
        }
        acc
    }

    /// The work of one thread. It owns the blocks `blocks` and returns its part of `x`.
    /// Only thread 0 gets `progress`.
    fn team_member(
        &self,
        a: &Matrix,
        rhs: &[f64],
        blocks: std::ops::Range<usize>,
        shared: &Shared,
        mut progress: Option<&mut dyn FnMut(usize, f64) -> bool>,
    ) -> Result<(Vec<f64>, usize), SolverError> {
        let n = a.n;
        let rows = blocks.start * BLOCK..(blocks.end * BLOCK).min(n);
        let len = rows.len();
        let mut x = vec![0.0; len];
        let mut r = rhs[rows.clone()].to_vec();
        let mut z: Vec<f64> = r
            .iter()
            .zip(&self.inv_diag[rows.clone()])
            .map(|(r, d)| r * d)
            .collect();
        let mut q = vec![0.0; len];
        let p = &shared.p;
        let get = |i: usize| f64::from_bits(p[i].load(Ordering::Relaxed));
        let mut rz = shared.rz0;
        for it in 1..=self.max_iterations {
            // q = A p, and the block sums of p . q.
            for b in blocks.clone() {
                let mut pq = 0.0;
                for i in b * BLOCK..((b + 1) * BLOCK).min(n) {
                    let qi = self.row(a, i, p);
                    q[i - rows.start] = qi;
                    pq += get(i) * qi;
                }
                shared.part[0][b].store(pq.to_bits(), Ordering::Relaxed);
            }
            shared.barrier.wait();
            let pq = shared.sum(0);
            if pq <= 0.0 || !pq.is_finite() {
                return Err(not_spd());
            }
            let alpha = rz / pq;
            // x, r and z, and the block sums of r . r and r . z.
            for b in blocks.clone() {
                let (mut rr, mut rzb) = (0.0, 0.0);
                for i in b * BLOCK..((b + 1) * BLOCK).min(n) {
                    let k = i - rows.start;
                    x[k] += alpha * get(i);
                    r[k] -= alpha * q[k];
                    z[k] = r[k] * self.inv_diag[i];
                    rr += r[k] * r[k];
                    rzb += r[k] * z[k];
                }
                shared.part[1][b].store(rr.to_bits(), Ordering::Relaxed);
                shared.part[2][b].store(rzb.to_bits(), Ordering::Relaxed);
            }
            shared.barrier.wait();
            let rel = shared.sum(1).sqrt() / shared.b_norm;
            if it % PROGRESS_EVERY == 0 {
                if let Some(f) = progress.as_mut() {
                    if !f(it, rel) {
                        shared.stop.store(true, Ordering::Relaxed);
                    }
                }
                shared.barrier.wait();
                if shared.stop.load(Ordering::Relaxed) {
                    return Err(SolverError::new("cancelled"));
                }
            }
            if rel <= self.tolerance {
                return Ok((x, it));
            }
            let rz_new = shared.sum(2);
            let beta = rz_new / rz;
            rz = rz_new;
            // Every thread read p in the product above, so the owner can change it now.
            for (k, i) in rows.clone().enumerate() {
                p[i].store((z[k] + beta * get(i)).to_bits(), Ordering::Relaxed);
            }
            shared.barrier.wait();
        }
        Err(SolverError::new(format!(
            "the conjugate gradient solver did not reach the tolerance {:e} in {} iterations",
            self.tolerance, self.max_iterations
        )))
    }
}

/// The data that the threads share. A barrier separates the writes and the reads.
struct Shared {
    /// The search direction, as the bits of each number.
    p: Vec<AtomicU64>,
    /// Block sums of p . q, r . r and r . z.
    part: [Vec<AtomicU64>; 3],
    barrier: Barrier,
    stop: AtomicBool,
    b_norm: f64,
    rz0: f64,
}

impl Shared {
    /// The sum of the block sums `k`, in block order.
    fn sum(&self, k: usize) -> f64 {
        self.part[k]
            .iter()
            .map(|v| f64::from_bits(v.load(Ordering::Relaxed)))
            .sum()
    }
}

/// The sum of `f(i)` for each block of `0..n`, then of the block sums in block order: the
/// same order as the team uses.
fn block_sum(n: usize, f: impl Fn(usize) -> f64) -> f64 {
    (0..n.div_ceil(BLOCK))
        .map(|b| (b * BLOCK..((b + 1) * BLOCK).min(n)).map(&f).sum::<f64>())
        .sum()
}

/// The first block of each thread: about the same number of matrix entries for each.
fn split(solver: &PcgSolver, a: &Matrix, threads: usize) -> Vec<usize> {
    let n_blocks = a.n.div_ceil(BLOCK);
    let entries = |b: usize| {
        let end = ((b + 1) * BLOCK).min(a.n);
        (solver.ptr[end] - solver.ptr[b * BLOCK]) as usize
            + (a.col_ptr[end] - a.col_ptr[b * BLOCK]) as usize
    };
    let total: usize = (0..n_blocks).map(entries).sum();
    let mut first = vec![0];
    let mut acc = 0;
    for b in 0..n_blocks {
        if acc * threads >= first.len() * total && first.len() < threads {
            first.push(b);
        }
        acc += entries(b);
    }
    first.push(n_blocks);
    first.dedup();
    first
}

impl LinearSolver for PcgSolver {
    fn factor(&mut self, a: &Matrix) -> Result<(), SolverError> {
        let n = a.n;
        let diag = a.diagonal();
        if diag.iter().any(|d| *d <= 0.0 || !d.is_finite()) {
            return Err(not_spd());
        }
        self.inv_diag = diag.iter().map(|d| 1.0 / d).collect();
        // Count the strict lower entries of each row, then fill the rows column by column,
        // so each row is in column order.
        self.ptr.clear();
        self.ptr.resize(n + 1, 0);
        for j in 0..n {
            for p in a.col_ptr[j] as usize + 1..a.col_ptr[j + 1] as usize {
                self.ptr[a.row_idx[p] as usize + 1] += 1;
            }
        }
        for i in 0..n {
            self.ptr[i + 1] += self.ptr[i];
        }
        let total = self.ptr[n] as usize;
        self.col.clear();
        self.col.resize(total, 0);
        self.val.clear();
        self.val.resize(total, 0.0);
        let mut next: Vec<u32> = self.ptr[..n].to_vec();
        for j in 0..n {
            for p in a.col_ptr[j] as usize + 1..a.col_ptr[j + 1] as usize {
                let i = a.row_idx[p] as usize;
                let at = next[i] as usize;
                self.col[at] = j as u32;
                self.val[at] = a.values[p];
                next[i] += 1;
            }
        }
        Ok(())
    }

    fn solve(
        &self,
        a: &Matrix,
        rhs: &[f64],
        progress: &mut dyn FnMut(usize, f64) -> bool,
    ) -> Result<(Vec<f64>, usize), SolverError> {
        let n = a.n;
        if self.inv_diag.len() != n || self.ptr.len() != n + 1 {
            return Err(SolverError::new("the matrix is not factored"));
        }
        let b_norm = block_sum(n, |i| rhs[i] * rhs[i]).sqrt();
        if b_norm == 0.0 {
            return Ok((vec![0.0; n], 0));
        }
        let rz0 = block_sum(n, |i| rhs[i] * rhs[i] * self.inv_diag[i]);
        let first = split(self, a, self.threads.min(n.div_ceil(BLOCK)));
        let team = first.len() - 1;
        let n_blocks = n.div_ceil(BLOCK);
        let shared = Shared {
            p: (0..n)
                .map(|i| AtomicU64::new((rhs[i] * self.inv_diag[i]).to_bits()))
                .collect(),
            part: std::array::from_fn(|_| (0..n_blocks).map(|_| AtomicU64::new(0)).collect()),
            barrier: Barrier::new(team),
            stop: AtomicBool::new(false),
            b_norm,
            rz0,
        };
        let parts = std::thread::scope(|s| {
            let others: Vec<_> = (1..team)
                .map(|t| {
                    let (shared, blocks) = (&shared, first[t]..first[t + 1]);
                    s.spawn(move || self.team_member(a, rhs, blocks, shared, None))
                })
                .collect();
            let mut parts =
                vec![self.team_member(a, rhs, first[0]..first[1], &shared, Some(progress))];
            for h in others {
                parts.push(h.join().unwrap_or_else(|_| {
                    Err(SolverError::new("a conjugate gradient thread failed"))
                }));
            }
            parts
        });
        let mut x = Vec::with_capacity(n);
        let mut iterations = 0;
        for part in parts {
            let (xt, it) = part?;
            x.extend_from_slice(&xt);
            iterations = it;
        }
        Ok((x, iterations))
    }
}
