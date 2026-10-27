//! Conjugate gradients with a geometric multigrid preconditioner on a rectilinear grid.
//!
//! Each coarser level keeps every other grid plane of each axis (and the last plane). The
//! prolongation P is a tensor product of 1D interpolations from the coarse grid points to the
//! fine ones. The weights of a point between 2 coarse planes come from the matrix: its couplings
//! to the 2 planes, summed over the other axes (the collapsed stencil). Thus the interpolation
//! follows a jump of the conductivity, for example at a thin layer of glue between 2 metals.
//! The geometric weights of the graded planes are the fallback. The coarse matrix is the Galerkin product
//! P^T A P, so it needs no coarse model and holds the jumps of the material.
//!
//! A displacement has 3 unknowns at each point, and keeps the linear weights, which hold the
//! rigid-body modes exactly. Its coarse matrix comes from the cells, not from P^T A P: the
//! coarse fields of a coarse cell are the 8 trilinear corner fields plus the 9 incompatible modes
//! of the coarse cell (`hex`), and their energy is the sum of the energies of the fine cells in
//! it, with the fine matrices. Static condensation removes the modes. The trilinear fields alone
//! lock in the bending of a thin part, as a plain 8-node hex does, and then the PCG iterations
//! grow with the length of a coarse cell over the thickness (2 times for each half of the
//! thickness, on a plate). With the modes, a coarse cell bends as the fine ones do.
//!
//! The smoother is
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
//! Methods for Sparse Linear Systems, 2nd ed., SIAM 2003, algorithm 9.1 (preconditioned CG);
//! E. L. Wilson, R. L. Taylor, W. P. Doherty and J. Ghaboussi, Incompatible displacement models,
//! 1973, and R. L. Taylor, P. J. Beresford and E. L. Wilson, A non-conforming element for stress
//! analysis, Int. J. Numer. Meth. Engng 10 (1976) 1211-1219 (the modes of the coarse cells);
//! D. Braess, Finite Elements, 3rd ed., Cambridge 2007, chapter V (multigrid with coarse
//! matrices from a discretization on the coarse grid, and the robustness for thin parts).

use crate::cholesky::Cholesky;
use crate::hex::{shape, BITS};
use crate::order::nested_dissection;
use crate::sparse::{Csr, Rect};
use crate::symbolic::Symbolic;

/// A level with this many unknowns or less is solved directly.
const COARSEST: usize = 3000;

/// The unknowns of a grid level: the planes of each axis, and the grid point and the component
/// (0 for a scalar, 0 to 2 for a displacement) of each unknown.
#[derive(Debug, Clone)]
pub struct Level {
    pub planes: [Vec<f64>; 3],
    pub points: Vec<[u32; 3]>,
    pub comp: Vec<u8>,
}

/// The origin and the 24 x 24 matrix of each cell of a coarse level.
type CoarseCells = (Vec<[u32; 3]>, Vec<[f64; 576]>);

/// The cells of the fine level of a displacement model: the lowest grid point of each cell, and
/// its element matrix (24 x 24, row-major, the unknown 3 n + c for corner n in the order of
/// `hex::BITS` and component c).
pub struct Cells<'a> {
    pub origin: Vec<[u32; 3]>,
    pub element: &'a dyn Fn(usize) -> [f64; 576],
}

/// The hierarchy of the matrices and prolongations, and the factor of the coarsest matrix.
pub struct Multigrid {
    a: Vec<Csr>,
    p: Vec<Rect>,
    /// The lines of the smoother along each axis, of each level but the coarsest.
    lines: Vec<[Lines; 3]>,
    coarse: Cholesky,
}

impl Multigrid {
    /// The hierarchy of the matrix `a` whose unknown `k` is the grid point `level.points[k]`.
    /// With `cells` (a displacement), the coarse matrices come from the cells.
    pub fn new(a: Csr, level: Level, cells: Option<Cells>) -> Result<Multigrid, String> {
        let mut mats = vec![a];
        let mut ps = Vec::new();
        let mut lines = Vec::new();
        let mut level = level;
        // The cells of the last coarse level and their matrices.
        // The origin and the matrix of each coarse cell of the last level.
        let mut stored: Option<CoarseCells> = None;
        loop {
            let a = mats.last().expect("1 matrix");
            if a.n <= COARSEST || ps.len() >= 30 {
                break;
            }
            let Some((p, coarse, keep)) = coarsen(&level, a) else {
                break;
            };
            if coarse.points.len() * 10 > a.n * 9 {
                break;
            }
            lines.push([
                Lines::new(&level, 0, a)?,
                Lines::new(&level, 1, a)?,
                Lines::new(&level, 2, a)?,
            ]);
            let ac = match (&cells, &stored) {
                (None, _) => p.galerkin(a),
                (Some(c), None) => {
                    let (ac, o, k) = by_cells(&level, &c.origin, c.element, &coarse, &keep);
                    stored = Some((o, k));
                    ac
                }
                (Some(_), Some((origin, k))) => {
                    let (ac, o, k) = by_cells(&level, origin, &|i| k[i], &coarse, &keep);
                    stored = Some((o, k));
                    ac
                }
            };
            mats.push(ac);
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
            lines.sweep(a, b, x, true);
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
            lines.sweep(a, b, x, false);
        }
    }
}

/// The lines of unknowns along 1 axis, and the band Cholesky factor of the matrix of each line.
/// A line holds the unknowns of 1 row of grid points along the axis, in order, split where a
/// point has no unknown. A point with 3 components puts its 3 unknowns side by side. A thin part
/// couples its points across its thickness much more than along it, and a line solve across the
/// thickness smooths the error that a point smoother leaves.
struct Lines {
    /// The unknowns, line by line: line `l` is `order[start[l]..start[l + 1]]`.
    order: Vec<u32>,
    start: Vec<usize>,
    /// The line of each unknown and its place in `order`.
    line: Vec<u32>,
    place: Vec<u32>,
    /// The half bandwidth: the largest distance in a line of 2 unknowns that couple.
    band: usize,
    /// Row r of the factor of its line, columns r - band to r, at `factor[r * (band + 1)..]`.
    factor: Vec<f64>,
}

impl Lines {
    fn new(level: &Level, axis: usize, a: &Csr) -> Result<Lines, String> {
        let (u, v) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let n = level.points.len();
        let mut order: Vec<u32> = (0..n as u32).collect();
        let key = |k: &u32| {
            let p = level.points[*k as usize];
            (p[v], p[u], p[axis], level.comp[*k as usize])
        };
        order.sort_unstable_by_key(key);
        let mut start = Vec::new();
        let mut last: Option<[u32; 3]> = None;
        for (r, &k) in order.iter().enumerate() {
            let p = level.points[k as usize];
            let next = last.is_some_and(|q| {
                q[u] == p[u] && q[v] == p[v] && (q[axis] == p[axis] || q[axis] + 1 == p[axis])
            });
            if !next {
                start.push(r);
            }
            last = Some(p);
        }
        start.push(n);
        let mut line = vec![0u32; n];
        let mut place = vec![0u32; n];
        for l in 0..start.len() - 1 {
            for r in start[l]..start[l + 1] {
                line[order[r] as usize] = l as u32;
                place[order[r] as usize] = r as u32;
            }
        }
        // The 2 points next to each other on a line couple, and no others: the band holds the
        // unknowns of 2 points.
        let comps = level
            .comp
            .iter()
            .map(|&c| c as usize + 1)
            .max()
            .unwrap_or(1);
        let band = 2 * comps - 1;
        let w = band + 1;
        let mut factor = vec![0.0; n * w];
        for l in 0..start.len() - 1 {
            for r in start[l]..start[l + 1] {
                let i = order[r] as usize;
                let first = r.saturating_sub(band).max(start[l]);
                for c in first..=r {
                    let mut s = a.find(i, order[c] as usize).map_or(0.0, |e| a.val[e]);
                    for k in first.max(c.saturating_sub(band))..c {
                        s -= factor[r * w + band + k - r] * factor[c * w + band + k - c];
                    }
                    if c == r {
                        if !(s > 0.0) {
                            return Err("the matrix is not positive definite: a part can move \
                                        with no strain"
                                .into());
                        }
                        factor[r * w + band] = s.sqrt();
                    } else {
                        factor[r * w + band + c - r] = s / factor[c * w + band];
                    }
                }
            }
        }
        Ok(Lines {
            order,
            start,
            line,
            place,
            band,
            factor,
        })
    }

    /// 1 line Gauss-Seidel sweep of A x = b, forward or backward: each line solves its band
    /// system with the other unknowns at their last values.
    fn sweep(&self, a: &Csr, b: &[f64], x: &mut [f64], forward: bool) {
        let (band, w) = (self.band, self.band + 1);
        let mut y = Vec::new();
        let mut solve = |l: usize, x: &mut [f64]| {
            let (first, end) = (self.start[l], self.start[l + 1]);
            y.clear();
            for r in first..end {
                let i = self.order[r] as usize;
                let mut s = b[i];
                for k in a.ptr[i]..a.ptr[i + 1] {
                    let j = a.idx[k] as usize;
                    let near =
                        self.line[j] as usize == l && (self.place[j] as usize).abs_diff(r) <= band;
                    if !near {
                        s -= a.val[k] * x[j];
                    }
                }
                y.push(s);
            }
            // L y = s, then L^T x = y.
            for r in first..end {
                let mut s = y[r - first];
                for k in r.saturating_sub(band).max(first)..r {
                    s -= self.factor[r * w + band + k - r] * y[k - first];
                }
                y[r - first] = s / self.factor[r * w + band];
            }
            for r in (first..end).rev() {
                let mut s = y[r - first];
                for k in r + 1..(r + w).min(end) {
                    s -= self.factor[k * w + band + r - k] * y[k - first];
                }
                y[r - first] = s / self.factor[r * w + band];
            }
            for r in first..end {
                x[self.order[r] as usize] = y[r - first];
            }
        };
        let lines = self.start.len() - 1;
        if forward {
            (0..lines).for_each(|l| solve(l, x));
        } else {
            (0..lines).rev().for_each(|l| solve(l, x));
        }
    }
}

/// The next coarser level, the prolongation to `fine` and the fine plane of each coarse plane
/// of each axis. `None` when no axis has more than 2 planes.
fn coarsen(fine: &Level, a: &Csr) -> Option<(Rect, Level, [Vec<usize>; 3])> {
    let mut kept: [Vec<usize>; 3] = Default::default();
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
        kept[d] = keep.clone();
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
    let comps = fine.comp.iter().map(|&c| c as usize + 1).max().unwrap_or(1);
    let flat = |q: [u32; 3], c: u8| {
        c as usize + comps * (q[0] as usize + dims[0] * (q[1] as usize + dims[1] * q[2] as usize))
    };
    let mut number = vec![u32::MAX; comps * dims[0] * dims[1] * dims[2]];
    let mut points = Vec::new();
    let mut comp = Vec::new();
    let mut ptr = vec![0];
    let (mut idx, mut val) = (Vec::new(), Vec::new());
    for (row, f) in fine.points.iter().enumerate() {
        let start = idx.len();
        let c = fine.comp[row];
        let weights: [[(u32, f64); 2]; 3] = std::array::from_fn(|d| {
            let mut w = map[d][f[d] as usize];
            // A displacement keeps the linear weights: they hold each linear field, so the
            // rigid-body modes, exactly.
            if w[0].0 != w[1].0 && comps == 1 {
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
                    let slot = &mut number[flat(q, c)];
                    if *slot == u32::MAX {
                        *slot = points.len() as u32;
                        points.push(q);
                        comp.push(c);
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
    Some((
        p,
        Level {
            planes,
            points,
            comp,
        },
        kept,
    ))
}

/// The coarse matrix of a displacement from the cells: each coarse cell sums the energy of its
/// fine cells (`origin`, matrices from `element`) for its 8 trilinear corner fields and its 9
/// incompatible modes, on the free fine unknowns, and condenses the modes. Also the lowest point
/// and the matrix of each coarse cell, for the next level.
fn by_cells(
    fine: &Level,
    origin: &[[u32; 3]],
    element: &dyn Fn(usize) -> [f64; 576],
    coarse: &Level,
    keep: &[Vec<usize>; 3],
) -> (Csr, Vec<[u32; 3]>, Vec<[f64; 576]>) {
    let fd = fine.planes.each_ref().map(Vec::len);
    let cd = coarse.planes.each_ref().map(Vec::len);
    let index = |q: [u32; 3], c: usize, d: [usize; 3]| {
        c + 3 * (q[0] as usize + d[0] * (q[1] as usize + d[1] * q[2] as usize))
    };
    let mut free = vec![false; 3 * fd[0] * fd[1] * fd[2]];
    for (p, &c) in fine.points.iter().zip(&fine.comp) {
        free[index(*p, c as usize, fd)] = true;
    }
    let mut number = vec![u32::MAX; 3 * cd[0] * cd[1] * cd[2]];
    for (k, (p, &c)) in coarse.points.iter().zip(&coarse.comp).enumerate() {
        number[index(*p, c as usize, cd)] = k as u32;
    }
    // The coarse cell of each fine cell along each axis.
    let interval: [Vec<u32>; 3] = std::array::from_fn(|d| {
        let mut out = Vec::with_capacity(fd[d]);
        let mut c = 0;
        for i in 0..fd[d] {
            while c + 2 < keep[d].len() && keep[d][c + 1] <= i {
                c += 1;
            }
            out.push(c as u32);
        }
        out
    });
    let mut order: Vec<(u32, [u32; 3])> = origin
        .iter()
        .enumerate()
        .map(|(f, o)| {
            (
                f as u32,
                std::array::from_fn(|d| interval[d][o[d] as usize]),
            )
        })
        .collect();
    order.sort_unstable_by_key(|(_, c)| (c[2], c[1], c[0]));
    let mut cells: Vec<[u32; 3]> = Vec::new();
    let mut matrices: Vec<[f64; 576]> = Vec::new();
    let mut start = 0;
    while start < order.len() {
        let cc = order[start].1;
        let mut end = start;
        while end < order.len() && order[end].1 == cc {
            end += 1;
        }
        let lo: [f64; 3] = std::array::from_fn(|d| coarse.planes[d][cc[d] as usize]);
        let hi: [f64; 3] = std::array::from_fn(|d| coarse.planes[d][cc[d] as usize + 1]);
        // The 33 x 33 matrix of the 11 coarse functions times 3 components: 3 a + c, and the
        // Gram matrix of the 11 functions at the free fine unknowns of each component.
        let mut k = vec![0.0; 33 * 33];
        let mut gram = [[[0.0; 11]; 11]; 3];
        for &(f, _) in &order[start..end] {
            let kf = element(f as usize);
            let o = origin[f as usize];
            // The coarse functions at the 8 fine corners, 0 at a fixed fine unknown.
            let mut phi = [[[0.0; 11]; 3]; 8];
            for (n, b) in BITS.iter().enumerate() {
                let q: [u32; 3] = std::array::from_fn(|d| o[d] + b[d] as u32);
                let xi: [f64; 3] = std::array::from_fn(|d| {
                    2.0 * (fine.planes[d][q[d] as usize] - lo[d]) / (hi[d] - lo[d]) - 1.0
                });
                let corner = shape(xi);
                for c in 0..3 {
                    if free[index(q, c, fd)] {
                        let f = &mut phi[n][c];
                        f[..8].copy_from_slice(&corner);
                        for d in 0..3 {
                            f[8 + d] = 1.0 - xi[d] * xi[d];
                        }
                        for a in 0..11 {
                            for b in 0..11 {
                                gram[c][a][b] += f[a] * f[b];
                            }
                        }
                    }
                }
            }
            // kt = Kf T, then K += T^T kt.
            let mut kt = [[0.0; 33]; 24];
            for r in 0..24 {
                for (m, pm) in phi.iter().enumerate() {
                    for c in 0..3 {
                        let v = kf[r * 24 + 3 * m + c];
                        if v != 0.0 {
                            for a in 0..11 {
                                kt[r][3 * a + c] += v * pm[c][a];
                            }
                        }
                    }
                }
            }
            for (n, pn) in phi.iter().enumerate() {
                for c in 0..3 {
                    let row = &kt[3 * n + c];
                    for a in 0..11 {
                        let w = pn[c][a];
                        if w != 0.0 {
                            let out = &mut k[(3 * a + c) * 33..(3 * a + c + 1) * 33];
                            for (o, v) in out.iter_mut().zip(row) {
                                *o += w * v;
                            }
                        }
                    }
                }
            }
        }
        matrices.push(condense(&mut k, &modes(&gram)));
        cells.push(cc);
        start = end;
    }
    // Assemble on the coarse unknowns.
    let ids: Vec<Vec<(usize, u32)>> = cells
        .iter()
        .map(|o| {
            let mut v = Vec::new();
            for (n, b) in BITS.iter().enumerate() {
                let q: [u32; 3] = std::array::from_fn(|d| o[d] + b[d] as u32);
                for c in 0..3 {
                    let g = number[index(q, c, cd)];
                    if g != u32::MAX {
                        v.push((3 * n + c, g));
                    }
                }
            }
            v
        })
        .collect();
    let globals: Vec<Vec<u32>> = ids
        .iter()
        .map(|v| v.iter().map(|&(_, g)| g).collect())
        .collect();
    let mut a = Csr::pattern(coarse.points.len(), globals.iter().map(|v| &v[..]));
    let mut ke = Vec::new();
    for (e, list) in ids.iter().enumerate() {
        ke.clear();
        for &(r, _) in list {
            for &(c, _) in list {
                ke.push(matrices[e][r * 24 + c]);
            }
        }
        a.add_element(&globals[e], &ke);
    }
    (a, cells, matrices)
}

/// The modes of a coarse cell to keep, 3 d + c for the mode along axis d of component c: those
/// that are not in the span of the corner fields at the free fine unknowns. A corner whose own
/// fine point is fixed or void reaches the fine grid only at points between the corners, where a
/// mode can cancel it: with that mode the cell has a motion with no energy that is not rigid,
/// and the coarse matrix is singular. The test is a Cholesky factor of the Gram matrix of the 11
/// functions, corners first: a mode whose pivot is a small part of its diagonal is out.
fn modes(gram: &[[[f64; 11]; 11]; 3]) -> [bool; 9] {
    let mut keep = [false; 9];
    for (c, g) in gram.iter().enumerate() {
        let mut l = [[0.0; 11]; 11];
        let mut used = [false; 11];
        for j in 0..11 {
            let mut d = g[j][j];
            for k in 0..j {
                if used[k] {
                    d -= l[j][k] * l[j][k];
                }
            }
            if !(d > 1e-6 * g[j][j]) {
                continue;
            }
            used[j] = true;
            let d = d.sqrt();
            l[j][j] = d;
            for i in j + 1..11 {
                let mut v = g[i][j];
                for k in 0..j {
                    if used[k] {
                        v -= l[i][k] * l[j][k];
                    }
                }
                l[i][j] = v / d;
            }
            if j >= 8 {
                keep[3 * (j - 8) + c] = true;
            }
        }
    }
    keep
}

/// The 24 x 24 matrix of the corners from the 33 x 33 matrix of the corners and the 9 modes
/// (`k`, row-major, which it changes): symmetric Gauss elimination of the modes in `keep`. A mode
/// with no energy is left out too.
fn condense(k: &mut [f64], keep: &[bool; 9]) -> [f64; 576] {
    for m in 24..33 {
        if !keep[m - 24] {
            continue;
        }
        let pivot = k[m * 33 + m];
        let scale = (0..33).map(|i| k[i * 33 + i].abs()).fold(0.0, f64::max);
        if !(pivot > 1e-12 * scale) {
            continue;
        }
        for i in 0..33 {
            let f = k[i * 33 + m] / pivot;
            if i == m || f == 0.0 {
                continue;
            }
            for j in 0..33 {
                k[i * 33 + j] -= f * k[m * 33 + j];
            }
        }
    }
    let mut out = [0.0; 576];
    for r in 0..24 {
        out[r * 24..(r + 1) * 24].copy_from_slice(&k[r * 33..r * 33 + 24]);
    }
    out
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
