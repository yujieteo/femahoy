//! A fill-reducing ordering: nested dissection with coordinate bisection.
//!
//! The ordering splits the nodes at the median coordinate of the longest side of their bounding
//! box. The nodes of the smaller side that couple to the other side are the separator. Each side
//! is ordered first, by the same rule, and the separator last. For a regular grid of n nodes in
//! 3D this gives O(n^(4/3)) entries in the factor and O(n^2) operations.
//!
//! Sources: A. George, Nested dissection of a regular finite element mesh, SIAM J. Numer. Anal.
//! 10 (1973) 345-363; A. George and J. W. H. Liu, Computer Solution of Large Sparse Positive
//! Definite Systems, Prentice-Hall 1981, chapter 8.

use crate::sparse::Csr;

/// A set of this size or less is not split again.
const LEAF: usize = 64;

/// The nested dissection order of the nodes of the symmetric pattern `a`, from the coordinates
/// `xyz` of each node. Item `k` of the result is the old index of the new node `k`.
pub fn nested_dissection(a: &Csr, xyz: &[[f64; 3]]) -> Vec<usize> {
    assert_eq!(a.n, xyz.len(), "1 point for each node");
    let mut order = Vec::with_capacity(a.n);
    // 0: not in the current set, 1: side A, 2: side B.
    let mut side = vec![0u8; a.n];
    let all: Vec<usize> = (0..a.n).collect();
    // Each item is a set to order, or a separator to put last (after the sets above it).
    enum Work {
        Split(Vec<usize>),
        Emit(Vec<usize>),
    }
    let mut stack = vec![Work::Split(all)];
    while let Some(work) = stack.pop() {
        let set = match work {
            Work::Emit(sep) => {
                order.extend(sep);
                continue;
            }
            Work::Split(set) => set,
        };
        if set.len() <= LEAF {
            order.extend(set);
            continue;
        }
        let Some((left, right)) = bisect(&set, xyz) else {
            order.extend(set);
            continue;
        };
        for &v in &left {
            side[v] = 1;
        }
        for &v in &right {
            side[v] = 2;
        }
        let touches = |v: usize, other: u8| {
            a.idx[a.ptr[v]..a.ptr[v + 1]]
                .iter()
                .any(|&w| side[w as usize] == other)
        };
        let sep_left: Vec<usize> = left.iter().copied().filter(|&v| touches(v, 2)).collect();
        let sep_right: Vec<usize> = right.iter().copied().filter(|&v| touches(v, 1)).collect();
        for &v in &set {
            side[v] = 0;
        }
        let (sep, from_left) = if sep_left.len() <= sep_right.len() {
            (sep_left, true)
        } else {
            (sep_right, false)
        };
        let mut in_sep = sep.clone();
        in_sep.sort_unstable();
        let rest = |part: Vec<usize>| -> Vec<usize> {
            part.into_iter()
                .filter(|v| in_sep.binary_search(v).is_err())
                .collect()
        };
        let (left, right) = if from_left {
            (rest(left), right)
        } else {
            (left, rest(right))
        };
        // The stack runs the last push first: left, then right, then the separator.
        stack.push(Work::Emit(sep));
        stack.push(Work::Split(right));
        stack.push(Work::Split(left));
    }
    order
}

/// Split `set` at the median coordinate of the longest side of its bounding box. `None` when all
/// the nodes are at 1 point.
fn bisect(set: &[usize], xyz: &[[f64; 3]]) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for &v in set {
        for d in 0..3 {
            lo[d] = lo[d].min(xyz[v][d]);
            hi[d] = hi[d].max(xyz[v][d]);
        }
    }
    let axis = (0..3)
        .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
        .expect("3 axes");
    if hi[axis] <= lo[axis] {
        return None;
    }
    let mut values: Vec<f64> = set.iter().map(|&v| xyz[v][axis]).collect();
    let mid = values.len() / 2;
    let (_, median, _) = values.select_nth_unstable_by(mid, f64::total_cmp);
    let mut cut = *median;
    if cut <= lo[axis] {
        // More than half of the nodes are on the lowest plane: cut above it.
        cut = values
            .iter()
            .copied()
            .filter(|&x| x > lo[axis])
            .fold(f64::INFINITY, f64::min);
    }
    let (left, right) = set.iter().partition(|&&v| xyz[v][axis] < cut);
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_is_a_permutation_with_the_middle_plane_last() {
        // A line of 200 nodes: the first split is at the middle.
        let n = 200;
        let elements: Vec<[u32; 2]> = (0..n as u32 - 1).map(|i| [i, i + 1]).collect();
        let a = Csr::pattern(n, elements.iter().map(|e| &e[..]));
        let xyz: Vec<[f64; 3]> = (0..n).map(|i| [i as f64, 0.0, 0.0]).collect();
        let order = nested_dissection(&a, &xyz);
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..n).collect::<Vec<_>>());
        assert_eq!(order.last(), Some(&99));
    }
}
