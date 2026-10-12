//! Sparse symmetric matrix in compressed column form, lower triangle only,
//! with 32-bit indices. The pattern is built once from the element
//! connectivity. The values are filled in place and can be filled again for
//! later Newton iterations. The pattern is shared by the direct solver and
//! the conjugate gradient solver.

use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMatRef, SymbolicSparseColMatRef};
use faer::{Mat, Side};

use crate::constraint::Transform;
use crate::SolverError;

/// Which node shares an element with which node. Compressed rows, 32-bit.
pub struct NodeGraph {
    ptr: Vec<usize>,
    adj: Vec<u32>,
}

impl NodeGraph {
    /// `conn` holds the node indices of the elements, `start` the first entry of each element.
    pub fn new(n_nodes: usize, conn: &[u32], start: &[u32]) -> Self {
        // Elements of each node.
        let mut count = vec![0_u32; n_nodes + 1];
        for &n in conn {
            count[n as usize + 1] += 1;
        }
        for i in 0..n_nodes {
            count[i + 1] += count[i];
        }
        let mut fill = count.clone();
        let mut node_el = vec![0_u32; conn.len()];
        for (e, w) in start.windows(2).enumerate() {
            for &n in &conn[w[0] as usize..w[1] as usize] {
                node_el[fill[n as usize] as usize] = e as u32;
                fill[n as usize] += 1;
            }
        }
        drop(fill);
        let mut ptr = Vec::with_capacity(n_nodes + 1);
        let mut adj: Vec<u32> = Vec::with_capacity(conn.len() * 4);
        ptr.push(0);
        let mut scratch: Vec<u32> = Vec::new();
        for n in 0..n_nodes {
            scratch.clear();
            for &e in &node_el[count[n] as usize..count[n + 1] as usize] {
                let w = (start[e as usize] as usize, start[e as usize + 1] as usize);
                scratch.extend_from_slice(&conn[w.0..w.1]);
            }
            scratch.sort_unstable();
            scratch.dedup();
            adj.extend_from_slice(&scratch);
            ptr.push(adj.len());
        }
        adj.shrink_to_fit();
        NodeGraph { ptr, adj }
    }

    /// The nodes that share an element with `n`, including `n` if it is in an element.
    pub fn neighbours(&self, n: usize) -> &[u32] {
        &self.adj[self.ptr[n]..self.ptr[n + 1]]
    }

    /// Sum of the neighbour counts.
    pub fn entries(&self) -> usize {
        self.adj.len()
    }

    pub fn bytes(&self) -> usize {
        self.ptr.len() * 8 + self.adj.len() * 4
    }
}

/// The matrix: pattern and values.
pub struct Matrix {
    pub n: usize,
    col_ptr: Vec<u32>,
    row_idx: Vec<u32>,
    pub values: Vec<f64>,
}

/// Call `f(col, row)` for every lower entry that `Tᵀ K T` can have, with
/// repeats. `dof_of(node, c)` gives the full dof of component `c` of a node.
fn for_each_entry(
    graph: &NodeGraph,
    dof_of: &dyn Fn(usize, usize) -> usize,
    t: &Transform,
    f: &mut dyn FnMut(u32, u32),
) {
    for m in 0..graph.ptr.len() - 1 {
        let nb = graph.neighbours(m);
        if nb.is_empty() {
            continue;
        }
        for cj in 0..3 {
            let (bs, _) = t.row(dof_of(m, cj));
            for &b in bs {
                for &n in nb {
                    for ci in 0..3 {
                        let (as_, _) = t.row(dof_of(n as usize, ci));
                        for &a in as_ {
                            if a >= b {
                                f(b, a);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Matrix {
    /// Build the pattern for `Tᵀ K T`. Every column has its diagonal entry.
    pub fn pattern(
        graph: &NodeGraph,
        dof_of: &dyn Fn(usize, usize) -> usize,
        t: &Transform,
    ) -> Result<Matrix, SolverError> {
        let n = t.n_free;
        let mut ptr = vec![0_u64; n + 1];
        for b in 0..n {
            ptr[b + 1] = 1;
        }
        for_each_entry(graph, dof_of, t, &mut |b, _| ptr[b as usize + 1] += 1);
        for b in 0..n {
            ptr[b + 1] += ptr[b];
        }
        if ptr[n] >= u64::from(u32::MAX) {
            return Err(SolverError::new(format!(
                "the matrix has {} entries, more than the 32-bit index allows",
                ptr[n]
            )));
        }
        let bound = ptr[n] as usize;
        let mut row_idx = vec![0_u32; bound];
        let mut fill: Vec<u32> = ptr[..n].iter().map(|&p| p as u32).collect();
        for (b, f) in fill.iter_mut().enumerate() {
            row_idx[*f as usize] = b as u32;
            *f += 1;
        }
        for_each_entry(graph, dof_of, t, &mut |b, a| {
            let slot = &mut fill[b as usize];
            row_idx[*slot as usize] = a;
            *slot += 1;
        });
        drop(fill);
        // Sort each column, drop repeats and close the gaps.
        let mut col_ptr = vec![0_u32; n + 1];
        let mut w = 0_usize;
        for b in 0..n {
            let (s, e) = (ptr[b] as usize, ptr[b + 1] as usize);
            row_idx[s..e].sort_unstable();
            col_ptr[b] = w as u32;
            let mut last = u32::MAX;
            for p in s..e {
                let r = row_idx[p];
                if r != last {
                    row_idx[w] = r;
                    w += 1;
                    last = r;
                }
            }
        }
        col_ptr[n] = w as u32;
        row_idx.truncate(w);
        row_idx.shrink_to_fit();
        Ok(Matrix {
            n,
            col_ptr,
            row_idx,
            values: vec![0.0; w],
        })
    }

    pub fn nnz(&self) -> usize {
        self.row_idx.len()
    }

    pub fn clear(&mut self) {
        self.values.fill(0.0);
    }

    /// Add `v` at row `a`, column `b`, with `a >= b`. The entry must be in the pattern.
    #[inline]
    pub fn add(&mut self, a: u32, b: u32, v: f64) {
        let (s, e) = (
            self.col_ptr[b as usize] as usize,
            self.col_ptr[b as usize + 1] as usize,
        );
        match self.row_idx[s..e].binary_search(&a) {
            Ok(p) => self.values[s + p] += v,
            Err(_) => debug_assert!(false, "entry ({a}, {b}) is not in the pattern"),
        }
    }

    /// The diagonal.
    pub fn diagonal(&self) -> Vec<f64> {
        // The diagonal is the first entry of each column.
        (0..self.n)
            .map(|b| self.values[self.col_ptr[b] as usize])
            .collect()
    }

    /// `y = A x` for the symmetric matrix.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        y.fill(0.0);
        for j in 0..self.n {
            let (s, e) = (self.col_ptr[j] as usize, self.col_ptr[j + 1] as usize);
            let xj = x[j];
            let mut acc = self.values[s] * xj;
            for p in s + 1..e {
                let i = self.row_idx[p] as usize;
                let v = self.values[p];
                y[i] += v * xj;
                acc += v * x[i];
            }
            y[j] += acc;
        }
    }

    /// Sparse Cholesky of the matrix (faer). The factor needs more memory than the matrix.
    pub fn cholesky_solve(&self, rhs: &[f64]) -> Result<Vec<f64>, SolverError> {
        let symbolic = SymbolicSparseColMatRef::new_checked(
            self.n,
            self.n,
            &self.col_ptr,
            None,
            &self.row_idx,
        );
        let a = SparseColMatRef::new(symbolic, &self.values);
        let llt = a.sp_cholesky(Side::Lower).map_err(|_| not_spd())?;
        let mut x = Mat::<f64>::from_fn(self.n, 1, |i, _| rhs[i]);
        llt.solve_in_place(x.as_mut());
        Ok((0..self.n).map(|i| x[(i, 0)]).collect())
    }

    /// Conjugate gradients with a Jacobi preconditioner. The sums run in a
    /// fixed order, so the result is the same on every run. `progress` gets
    /// the iteration count and the relative residual.
    pub fn pcg_solve(
        &self,
        rhs: &[f64],
        tolerance: f64,
        max_iterations: usize,
        progress: &mut dyn FnMut(usize, f64) -> bool,
    ) -> Result<(Vec<f64>, usize), SolverError> {
        let n = self.n;
        let inv_diag: Vec<f64> = self
            .diagonal()
            .into_iter()
            .map(|d| if d > 0.0 { 1.0 / d } else { 0.0 })
            .collect();
        if inv_diag.contains(&0.0) {
            return Err(not_spd());
        }
        let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
        let b_norm = dot(rhs, rhs).sqrt();
        let mut x = vec![0.0; n];
        if b_norm == 0.0 {
            return Ok((x, 0));
        }
        let mut r = rhs.to_vec();
        let mut z: Vec<f64> = r.iter().zip(&inv_diag).map(|(r, d)| r * d).collect();
        let mut p = z.clone();
        let mut q = vec![0.0; n];
        let mut rz = dot(&r, &z);
        for it in 1..=max_iterations {
            self.mul(&p, &mut q);
            let pq = dot(&p, &q);
            if pq <= 0.0 || !pq.is_finite() {
                return Err(not_spd());
            }
            let alpha = rz / pq;
            for i in 0..n {
                x[i] += alpha * p[i];
                r[i] -= alpha * q[i];
            }
            let rel = dot(&r, &r).sqrt() / b_norm;
            if it % 25 == 0 && !progress(it, rel) {
                return Err(SolverError::new("cancelled"));
            }
            if rel <= tolerance {
                return Ok((x, it));
            }
            for i in 0..n {
                z[i] = r[i] * inv_diag[i];
            }
            let rz_new = dot(&r, &z);
            let beta = rz_new / rz;
            rz = rz_new;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
        }
        Err(SolverError::new(format!(
            "the conjugate gradient solver did not reach the tolerance {tolerance:e} in {max_iterations} iterations"
        )))
    }
}

fn not_spd() -> SolverError {
    SolverError::new(
        "stiffness matrix is not positive definite: check the boundary conditions for rigid body motion",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Elimination;

    /// A chain of `n` nodes with 3 dofs. Element `e` joins nodes `e` and `e + 1`.
    fn chain(n: usize) -> (NodeGraph, Transform) {
        let conn: Vec<u32> = (0..n as u32 - 1).flat_map(|e| [e, e + 1]).collect();
        let start: Vec<u32> = (0..n as u32).map(|e| 2 * e).collect();
        (
            NodeGraph::new(n, &conn, &start),
            Elimination::new(3 * n).finish(),
        )
    }

    #[test]
    fn graph_lists_each_neighbour_once() {
        let (g, _) = chain(4);
        assert_eq!(g.neighbours(0), &[0, 1]);
        assert_eq!(g.neighbours(2), &[1, 2, 3]);
    }

    #[test]
    fn pattern_has_the_diagonal_and_sorted_unique_rows() {
        let (g, t) = chain(4);
        let m = Matrix::pattern(&g, &|n, c| 3 * n + c, &t).unwrap();
        // Column 0 (node 0, x): own 3 dofs plus 3 of node 1 = rows 0,1,2,3,4,5.
        assert_eq!(&m.row_idx[..6], &[0, 1, 2, 3, 4, 5]);
        for b in 0..m.n {
            let col = &m.row_idx[m.col_ptr[b] as usize..m.col_ptr[b + 1] as usize];
            assert_eq!(col[0] as usize, b);
            assert!(col.windows(2).all(|w| w[0] < w[1]));
        }
    }

    #[test]
    fn product_and_both_solvers_agree() {
        let (g, t) = chain(6);
        let mut m = Matrix::pattern(&g, &|n, c| 3 * n + c, &t).unwrap();
        // Spring chain with a grounded first node: K = sum of springs.
        for e in 0..5_u32 {
            for c in 0..3_u32 {
                let (i, j) = (3 * e + c, 3 * (e + 1) + c);
                m.add(i, i, 1.0);
                m.add(j, j, 1.0);
                m.add(j, i, -1.0);
            }
        }
        for c in 0..3 {
            m.add(c, c, 1.0);
        }
        let rhs: Vec<f64> = (0..18).map(|i| 1.0 + f64::from(i) * 0.1).collect();
        let direct = m.cholesky_solve(&rhs).unwrap();
        let (iter, its) = m.pcg_solve(&rhs, 1e-13, 1000, &mut |_, _| true).unwrap();
        assert!(its > 0);
        for (a, b) in direct.iter().zip(&iter) {
            assert!((a - b).abs() < 1e-9 * a.abs().max(1.0), "{a} {b}");
        }
        let mut y = vec![0.0; 18];
        m.mul(&direct, &mut y);
        for (a, b) in y.iter().zip(&rhs) {
            assert!((a - b).abs() < 1e-10, "{a} {b}");
        }
    }

    #[test]
    fn singular_matrix_is_reported_by_both_solvers() {
        let (g, t) = chain(3);
        let mut m = Matrix::pattern(&g, &|n, c| 3 * n + c, &t).unwrap();
        for e in 0..2_u32 {
            for c in 0..3_u32 {
                let (i, j) = (3 * e + c, 3 * (e + 1) + c);
                m.add(i, i, 1.0);
                m.add(j, j, 1.0);
                m.add(j, i, -1.0);
            }
        }
        let rhs = vec![1.0; 9];
        assert!(m
            .cholesky_solve(&rhs)
            .unwrap_err()
            .to_string()
            .contains("not positive definite"));
        let e = m.pcg_solve(&rhs, 1e-10, 200, &mut |_, _| true).unwrap_err();
        assert!(!e.to_string().is_empty());
    }
}
