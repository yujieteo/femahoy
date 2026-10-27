//! The symbolic analysis of a sparse symmetric factor: the elimination tree, its postorder, the
//! column counts, the supernodes and the row pattern of each supernode. It reads only the pattern
//! of the matrix, so each numeric factor of the same pattern uses it: the real Cholesky factor of
//! `cholesky`, and a later complex symmetric LDL^T.
//!
//! Sources: J. W. H. Liu, The role of elimination trees in sparse factorization, SIAM J. Matrix
//! Anal. Appl. 11 (1990) 134-172; T. Davis, Direct Methods for Sparse Linear Systems, SIAM 2006,
//! chapter 4 (elimination tree, postorder, row subtrees and column counts); J. W. H. Liu, E. G. Ng
//! and B. W. Peyton, On finding supernodes for sparse matrix computations, SIAM J. Matrix Anal.
//! Appl. 14 (1993) 242-252 (fundamental supernodes).

use crate::sparse::Csr;

pub(crate) const NONE: usize = usize::MAX;

/// The pattern of the factor L of P A P^T = L L^T (or L D L^T) in supernodes. A supernode is a
/// run of columns `start[s]..start[s + 1]` that share 1 row pattern below the diagonal block.
#[derive(Debug, Clone)]
pub struct Symbolic {
    pub n: usize,
    /// Item `k` is the old index of the new column `k`.
    pub perm: Vec<usize>,
    /// Item `i` is the new index of the old column `i`.
    pub iperm: Vec<usize>,
    /// The first column of each supernode, and `n` at the end.
    pub start: Vec<usize>,
    /// The rows of supernode `s` are `rows[rows_ptr[s]..rows_ptr[s + 1]]`, sorted. They start
    /// with the columns of the supernode.
    pub rows_ptr: Vec<usize>,
    pub rows: Vec<u32>,
    /// The parent supernode in the elimination tree, or `NONE` for a root. A parent has a larger
    /// index than its children.
    pub parent: Vec<usize>,
    /// The number of children of each supernode.
    pub children: Vec<usize>,
}

impl Symbolic {
    /// Analyse the symmetric pattern `a` (both triangles) in the fill-reducing order `order`
    /// (item `k` is the old index of the new column `k`).
    pub fn analyse(a: &Csr, order: &[usize]) -> Symbolic {
        let n = a.n;
        assert_eq!(order.len(), n, "the order has each column once");
        let inverse = |p: &[usize]| {
            let mut ip = vec![0; n];
            for (k, &i) in p.iter().enumerate() {
                ip[i] = k;
            }
            ip
        };
        let iorder = inverse(order);
        let parent = etree(a, order, &iorder);
        // Renumber the columns in a postorder of the tree: each subtree is then a run of columns,
        // and a parent comes after its children.
        let post = postorder(&parent);
        let perm: Vec<usize> = post.iter().map(|&k| order[k]).collect();
        let iperm = inverse(&perm);
        let ipost = inverse(&post);
        let parent: Vec<usize> = post
            .iter()
            .map(|&k| match parent[k] {
                NONE => NONE,
                p => ipost[p],
            })
            .collect();
        let counts = column_counts(a, &perm, &iperm, &parent);
        let mut nchild = vec![0usize; n];
        for &p in &parent {
            if p != NONE {
                nchild[p] += 1;
            }
        }
        // Fundamental supernodes: column j joins the supernode of j - 1 when j is the only child
        // of j - 1's parent and the patterns match.
        let mut start = vec![0];
        for j in 1..n {
            let joins = parent[j - 1] == j && counts[j - 1] == counts[j] + 1 && nchild[j] == 1;
            if !joins {
                start.push(j);
            }
        }
        start.push(n);
        let ns = start.len() - 1;
        let mut of_col = vec![0usize; n];
        for s in 0..ns {
            of_col[start[s]..start[s + 1]].fill(s);
        }
        let sn_parent: Vec<usize> = (0..ns)
            .map(|s| match parent[start[s + 1] - 1] {
                NONE => NONE,
                p => of_col[p],
            })
            .collect();
        let mut children = vec![0usize; ns];
        let mut kids: Vec<Vec<usize>> = vec![Vec::new(); ns];
        for s in 0..ns {
            if sn_parent[s] != NONE {
                children[sn_parent[s]] += 1;
                kids[sn_parent[s]].push(s);
            }
        }
        // The rows of each supernode: its columns, the rows of A below them, and the rows of its
        // children below their own columns.
        let mut rows_ptr = vec![0];
        let mut rows: Vec<u32> = Vec::new();
        let mut mark = vec![NONE; n];
        let mut list: Vec<u32> = Vec::new();
        for s in 0..ns {
            list.clear();
            let (first, end) = (start[s], start[s + 1]);
            for j in first..end {
                mark[j] = s;
                list.push(j as u32);
            }
            for j in first..end {
                let old = perm[j];
                for &c in &a.idx[a.ptr[old]..a.ptr[old + 1]] {
                    let i = iperm[c as usize];
                    if i >= end && mark[i] != s {
                        mark[i] = s;
                        list.push(i as u32);
                    }
                }
            }
            for &c in &kids[s] {
                for &i in &rows[rows_ptr[c]..rows_ptr[c + 1]] {
                    let i = i as usize;
                    if i >= end && mark[i] != s {
                        mark[i] = s;
                        list.push(i as u32);
                    }
                }
            }
            list.sort_unstable();
            debug_assert_eq!(list.len(), counts[first]);
            rows.extend_from_slice(&list);
            rows_ptr.push(rows.len());
        }
        Symbolic {
            n,
            perm,
            iperm,
            start,
            rows_ptr,
            rows,
            parent: sn_parent,
            children,
        }
    }

    /// The number of supernodes.
    pub fn supernodes(&self) -> usize {
        self.start.len() - 1
    }

    /// The rows of supernode `s`.
    pub fn rows_of(&self, s: usize) -> &[u32] {
        &self.rows[self.rows_ptr[s]..self.rows_ptr[s + 1]]
    }

    /// The number of entries of L, with the diagonal.
    pub fn entries(&self) -> u64 {
        (0..self.supernodes())
            .map(|s| {
                let k = (self.start[s + 1] - self.start[s]) as u64;
                let m = (self.rows_ptr[s + 1] - self.rows_ptr[s]) as u64;
                k * m - k * (k - 1) / 2
            })
            .sum()
    }

    /// The number of entries of the largest frontal matrix (the rows of a supernode, squared).
    pub fn largest_front(&self) -> u64 {
        (0..self.supernodes())
            .map(|s| (self.rows_ptr[s + 1] - self.rows_ptr[s]) as u64)
            .map(|m| m * m)
            .max()
            .unwrap_or(0)
    }
}

/// The elimination tree of P A P^T: item `k` is the parent of column `k`, or `NONE`.
fn etree(a: &Csr, order: &[usize], iorder: &[usize]) -> Vec<usize> {
    let n = a.n;
    let mut parent = vec![NONE; n];
    let mut ancestor = vec![NONE; n];
    for k in 0..n {
        let old = order[k];
        for &c in &a.idx[a.ptr[old]..a.ptr[old + 1]] {
            let mut i = iorder[c as usize];
            // Walk from i to the root of its subtree, with path compression to k.
            while i != NONE && i < k {
                let next = ancestor[i];
                ancestor[i] = k;
                if next == NONE {
                    parent[i] = k;
                }
                i = next;
            }
        }
    }
    parent
}

/// A postorder of the forest `parent`: item `k` is the column that comes at place `k`.
fn postorder(parent: &[usize]) -> Vec<usize> {
    let n = parent.len();
    // The children of each node in a linked list, in increasing order.
    let mut head = vec![NONE; n];
    let mut next = vec![NONE; n];
    for j in (0..n).rev() {
        if parent[j] != NONE {
            next[j] = head[parent[j]];
            head[parent[j]] = j;
        }
    }
    let mut post = Vec::with_capacity(n);
    let mut stack = Vec::new();
    for root in (0..n).filter(|&j| parent[j] == NONE) {
        stack.push(root);
        while let Some(&top) = stack.last() {
            let child = head[top];
            if child == NONE {
                stack.pop();
                post.push(top);
            } else {
                head[top] = next[child];
                stack.push(child);
            }
        }
    }
    post
}

/// The number of entries of each column of L, with the diagonal, from the row subtrees: row i of
/// L has the nodes of the tree on the paths from each k with A(i, k) != 0, k < i, up to i.
fn column_counts(a: &Csr, perm: &[usize], iperm: &[usize], parent: &[usize]) -> Vec<usize> {
    let n = a.n;
    let mut counts = vec![1usize; n];
    let mut mark = vec![NONE; n];
    for i in 0..n {
        mark[i] = i;
        let old = perm[i];
        for &c in &a.idx[a.ptr[old]..a.ptr[old + 1]] {
            let mut j = iperm[c as usize];
            while j < i && mark[j] != i {
                counts[j] += 1;
                mark[j] = i;
                j = parent[j];
            }
        }
    }
    counts
}
