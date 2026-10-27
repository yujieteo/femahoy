//! A sparse matrix in compressed sparse row (CSR) form, with both triangles of a symmetric matrix.
//!
//! Source: Y. Saad, Iterative Methods for Sparse Linear Systems, 2nd ed., SIAM 2003, section 3.4
//! (CSR storage) and T. Davis, Direct Methods for Sparse Linear Systems, SIAM 2006, chapter 2
//! (the sparse matrix product of Gustavson).

/// A square sparse matrix. Row `i` holds the columns `idx[ptr[i]..ptr[i + 1]]`, sorted, with the
/// values `val` at the same places.
#[derive(Debug, Clone, PartialEq)]
pub struct Csr {
    pub n: usize,
    pub ptr: Vec<usize>,
    pub idx: Vec<u32>,
    pub val: Vec<f64>,
}

impl Csr {
    /// The symmetric pattern of the elements `elements` (the nodes of each element) on `n` nodes:
    /// a node couples to each node of each element that holds it, and to itself. The values are 0.
    pub fn pattern<'a>(n: usize, elements: impl Iterator<Item = &'a [u32]> + Clone) -> Csr {
        // The elements of each node.
        let mut count = vec![0usize; n + 1];
        for e in elements.clone() {
            for &a in e {
                count[a as usize + 1] += 1;
            }
        }
        for i in 0..n {
            count[i + 1] += count[i];
        }
        let mut of_node = vec![0u32; count[n]];
        let mut next = count.clone();
        for (k, e) in elements.clone().enumerate() {
            for &a in e {
                of_node[next[a as usize]] = k as u32;
                next[a as usize] += 1;
            }
        }
        let list: Vec<&[u32]> = elements.collect();
        let mut ptr = Vec::with_capacity(n + 1);
        ptr.push(0);
        let mut idx = Vec::new();
        let mut mark = vec![usize::MAX; n];
        let mut row = Vec::new();
        for i in 0..n {
            row.clear();
            mark[i] = i;
            row.push(i as u32);
            for &k in &of_node[count[i]..count[i + 1]] {
                for &b in list[k as usize] {
                    if mark[b as usize] != i {
                        mark[b as usize] = i;
                        row.push(b);
                    }
                }
            }
            row.sort_unstable();
            idx.extend_from_slice(&row);
            ptr.push(idx.len());
        }
        let val = vec![0.0; idx.len()];
        Csr { n, ptr, idx, val }
    }

    /// The place of the entry (i, j) in `idx` and `val`, if the pattern has it.
    pub fn find(&self, i: usize, j: usize) -> Option<usize> {
        let row = &self.idx[self.ptr[i]..self.ptr[i + 1]];
        row.binary_search(&(j as u32)).ok().map(|k| self.ptr[i] + k)
    }

    /// Add the dense element matrix `ke` (row-major, `nodes.len()` square) at the rows and
    /// columns `nodes`. Each pair must be in the pattern.
    pub fn add_element(&mut self, nodes: &[u32], ke: &[f64]) {
        let m = nodes.len();
        for (a, &i) in nodes.iter().enumerate() {
            let row = &self.idx[self.ptr[i as usize]..self.ptr[i as usize + 1]];
            for (b, &j) in nodes.iter().enumerate() {
                let k = row
                    .binary_search(&j)
                    .expect("the pattern holds each element pair");
                self.val[self.ptr[i as usize] + k] += ke[a * m + b];
            }
        }
    }

    /// The diagonal.
    pub fn diagonal(&self) -> Vec<f64> {
        (0..self.n)
            .map(|i| self.find(i, i).map_or(0.0, |k| self.val[k]))
            .collect()
    }

    /// y = A x.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        for (i, yi) in y.iter_mut().enumerate().take(self.n) {
            let mut s = 0.0;
            for k in self.ptr[i]..self.ptr[i + 1] {
                s += self.val[k] * x[self.idx[k] as usize];
            }
            *yi = s;
        }
    }

    /// The rows and columns `keep` of the matrix, in that order. `keep` holds each index once.
    pub fn submatrix(&self, keep: &[usize]) -> Csr {
        let mut new = vec![u32::MAX; self.n];
        for (k, &i) in keep.iter().enumerate() {
            new[i] = k as u32;
        }
        let mut ptr = Vec::with_capacity(keep.len() + 1);
        ptr.push(0);
        let (mut idx, mut val) = (Vec::new(), Vec::new());
        for &i in keep {
            let start = idx.len();
            for k in self.ptr[i]..self.ptr[i + 1] {
                let j = new[self.idx[k] as usize];
                if j != u32::MAX {
                    idx.push(j);
                    val.push(self.val[k]);
                }
            }
            sort_row(&mut idx[start..], &mut val[start..]);
            ptr.push(idx.len());
        }
        Csr {
            n: keep.len(),
            ptr,
            idx,
            val,
        }
    }
}

/// Sort the columns of a row, with their values.
fn sort_row(idx: &mut [u32], val: &mut [f64]) {
    if idx.windows(2).all(|w| w[0] < w[1]) {
        return;
    }
    let mut pairs: Vec<(u32, f64)> = idx.iter().copied().zip(val.iter().copied()).collect();
    pairs.sort_unstable_by_key(|p| p.0);
    for (k, (j, v)) in pairs.into_iter().enumerate() {
        idx[k] = j;
        val[k] = v;
    }
}

/// A rectangular sparse matrix in CSR form: `rows` rows of `cols` columns. The multigrid
/// prolongation is one.
#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    pub rows: usize,
    pub cols: usize,
    pub ptr: Vec<usize>,
    pub idx: Vec<u32>,
    pub val: Vec<f64>,
}

impl Rect {
    /// y = P x.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        for (i, yi) in y.iter_mut().enumerate().take(self.rows) {
            let mut s = 0.0;
            for k in self.ptr[i]..self.ptr[i + 1] {
                s += self.val[k] * x[self.idx[k] as usize];
            }
            *yi = s;
        }
    }

    /// y = P^T x.
    pub fn mul_transpose(&self, x: &[f64], y: &mut [f64]) {
        y[..self.cols].fill(0.0);
        for (i, xi) in x.iter().enumerate().take(self.rows) {
            for k in self.ptr[i]..self.ptr[i + 1] {
                y[self.idx[k] as usize] += self.val[k] * xi;
            }
        }
    }

    /// The Galerkin product P^T A P of the square matrix `a` (rows of P = size of `a`).
    pub fn galerkin(&self, a: &Csr) -> Csr {
        let nc = self.cols;
        // The transpose of P, row by row.
        let mut tptr = vec![0usize; nc + 1];
        for &j in &self.idx {
            tptr[j as usize + 1] += 1;
        }
        for j in 0..nc {
            tptr[j + 1] += tptr[j];
        }
        let mut tidx = vec![0u32; self.idx.len()];
        let mut tval = vec![0.0; self.idx.len()];
        let mut next = tptr.clone();
        for i in 0..self.rows {
            for k in self.ptr[i]..self.ptr[i + 1] {
                let j = self.idx[k] as usize;
                tidx[next[j]] = i as u32;
                tval[next[j]] = self.val[k];
                next[j] += 1;
            }
        }
        // Row c of P^T A P = sum over P(i, c) of row i of A P, Gustavson's method with a dense
        // accumulator.
        let mut acc = vec![0.0; nc];
        let mut mark = vec![usize::MAX; nc];
        let mut ptr = Vec::with_capacity(nc + 1);
        ptr.push(0);
        let (mut idx, mut val) = (Vec::new(), Vec::new());
        let mut row: Vec<u32> = Vec::new();
        for c in 0..nc {
            row.clear();
            for t in tptr[c]..tptr[c + 1] {
                let (i, pic) = (tidx[t] as usize, tval[t]);
                for ka in a.ptr[i]..a.ptr[i + 1] {
                    let (l, aval) = (a.idx[ka] as usize, a.val[ka] * pic);
                    for kp in self.ptr[l]..self.ptr[l + 1] {
                        let d = self.idx[kp] as usize;
                        if mark[d] != c {
                            mark[d] = c;
                            acc[d] = 0.0;
                            row.push(d as u32);
                        }
                        acc[d] += aval * self.val[kp];
                    }
                }
            }
            row.sort_unstable();
            for &d in &row {
                idx.push(d);
                val.push(acc[d as usize]);
            }
            ptr.push(idx.len());
        }
        Csr {
            n: nc,
            ptr,
            idx,
            val,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pattern_of_two_elements_couples_each_pair_of_their_nodes() {
        let elements: [&[u32]; 2] = [&[0, 1], &[1, 2]];
        let a = Csr::pattern(3, elements.iter().copied());
        assert_eq!(a.ptr, [0, 2, 5, 7]);
        assert_eq!(a.idx, [0, 1, 0, 1, 2, 1, 2]);
    }

    #[test]
    fn the_galerkin_product_of_a_1d_laplacian_is_a_coarse_laplacian() {
        // 3 fine nodes 0, 1, 2 with the coarse nodes 0 and 2: P = [1 0; 0.5 0.5; 0 1].
        let elements: [&[u32]; 2] = [&[0, 1], &[1, 2]];
        let mut a = Csr::pattern(3, elements.iter().copied());
        for e in elements {
            a.add_element(e, &[1.0, -1.0, -1.0, 1.0]);
        }
        let p = Rect {
            rows: 3,
            cols: 2,
            ptr: vec![0, 1, 3, 4],
            idx: vec![0, 0, 1, 1],
            val: vec![1.0, 0.5, 0.5, 1.0],
        };
        let c = p.galerkin(&a);
        assert_eq!(c.idx, [0, 1, 0, 1]);
        assert_eq!(c.val, [0.5, -0.5, -0.5, 0.5]);
    }
}
