//! The graded box grid of a list of parts.
//!
//! Each part is a box with its faces on planes of constant x, y or z. The grid has a plane at
//! each face of each part, so every part face lies on a grid plane. Between 2 such planes the
//! grid has graded cells: a cell is at most `max_cell` long, a part has at least `min_cells`
//! cells through each of its sides, and 2 cells side by side differ in length by at most the
//! factor `max_growth`. Each cell takes the part that holds its centre, the part with the
//! highest priority where parts overlap. A cell in no part is void, and is not in the model.
//!
//! The grading is a size function h(x): the target size at each grid plane grows linearly away
//! from it, and the cells of each interval are equal steps of the integral of 1 / h.
//!
//! Sources: P. J. Frey and P.-L. George, Mesh Generation, 2nd ed., Wiley 2008, chapter 4
//! (structured grids) and section 20.2 (size maps and the mesh of a line from a size function);
//! the rectilinear grid and the staircase material map of the finite difference method,
//! G. E. Forsythe and W. R. Wasow, Finite-Difference Methods for Partial Differential Equations,
//! Wiley 1960.

/// A part: a box of 1 material.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    /// The index of the material in the list of the model.
    pub material: usize,
    /// Where parts overlap, the part with the highest priority holds the cells.
    pub priority: i64,
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

/// The rules of the cell sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    /// The largest cell length.
    pub max_cell: f64,
    /// The largest length ratio of 2 cells side by side, above 1.
    pub max_growth: f64,
    /// The smallest number of cells through each side of each part.
    pub min_cells: usize,
}

/// A side of a cell: the axis (0 x, 1 y, 2 z) and the direction of the outer normal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Side {
    pub axis: usize,
    pub positive: bool,
}

impl Side {
    pub const ALL: [Side; 6] = [
        Side {
            axis: 0,
            positive: false,
        },
        Side {
            axis: 0,
            positive: true,
        },
        Side {
            axis: 1,
            positive: false,
        },
        Side {
            axis: 1,
            positive: true,
        },
        Side {
            axis: 2,
            positive: false,
        },
        Side {
            axis: 2,
            positive: true,
        },
    ];

    /// The name of the side: `x-`, `x+`, `y-`, `y+`, `z-` or `z+`.
    pub fn name(self) -> String {
        format!(
            "{}{}",
            ["x", "y", "z"][self.axis],
            if self.positive { "+" } else { "-" }
        )
    }

    pub fn parse(text: &str) -> Option<Side> {
        Side::ALL.into_iter().find(|s| s.name() == text)
    }
}

/// A face of the model: a side of a cell with no cell of the model beyond it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Face {
    pub cell: usize,
    /// The index of the side in `Side::ALL`.
    pub side: u8,
}

/// The cells of the grid that hold no part.
pub const VOID: u32 = u32::MAX;

/// The graded grid and the part of each cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// The coordinates of the grid planes of each axis, increasing.
    pub planes: [Vec<f64>; 3],
    /// The part of each cell, or `VOID`. Cell (i, j, k) is item i + nx (j + ny k).
    pub part: Vec<u32>,
}

impl Grid {
    /// The grid of `parts`. `extra` holds more planes of each axis, for example a plane of
    /// symmetry. An error tells why the parts give no grid.
    pub fn build(parts: &[Part], spacing: Spacing, extra: [&[f64]; 3]) -> Result<Grid, String> {
        if parts.is_empty() {
            return Err("the model has no part".into());
        }
        if !(spacing.max_cell > 0.0 && spacing.max_cell.is_finite()) {
            return Err("max_cell must be a number above 0".into());
        }
        if !(spacing.max_growth > 1.0 && spacing.max_growth.is_finite()) {
            return Err("max_growth must be a number above 1".into());
        }
        if spacing.min_cells == 0 {
            return Err("min_cells must be 1 or more".into());
        }
        for p in parts {
            for d in 0..3 {
                if !(p.lo[d].is_finite() && p.hi[d].is_finite() && p.lo[d] < p.hi[d]) {
                    return Err(format!(
                        "part {}: its {} range must be 2 finite numbers, the first smaller",
                        p.name,
                        ["x", "y", "z"][d]
                    ));
                }
            }
        }
        let mut planes: [Vec<f64>; 3] = Default::default();
        for d in 0..3 {
            planes[d] = axis_planes(parts, d, spacing, extra[d])?;
        }
        let mut grid = Grid {
            part: vec![VOID; (planes[0].len() - 1) * (planes[1].len() - 1) * (planes[2].len() - 1)],
            planes,
        };
        grid.assign(parts)?;
        Ok(grid)
    }

    /// The number of cells along each axis.
    pub fn dims(&self) -> [usize; 3] {
        [
            self.planes[0].len() - 1,
            self.planes[1].len() - 1,
            self.planes[2].len() - 1,
        ]
    }

    pub fn cell_index(&self, c: [usize; 3]) -> usize {
        let [nx, ny, _] = self.dims();
        c[0] + nx * (c[1] + ny * c[2])
    }

    pub fn cell_ijk(&self, cell: usize) -> [usize; 3] {
        let [nx, ny, _] = self.dims();
        [cell % nx, (cell / nx) % ny, cell / (nx * ny)]
    }

    /// The node index of the grid point (i, j, k), before the void is removed.
    pub fn point_index(&self, p: [usize; 3]) -> usize {
        let [nx, ny, _] = self.dims();
        p[0] + (nx + 1) * (p[1] + (ny + 1) * p[2])
    }

    pub fn point(&self, p: [usize; 3]) -> [f64; 3] {
        [
            self.planes[0][p[0]],
            self.planes[1][p[1]],
            self.planes[2][p[2]],
        ]
    }

    /// The centre of a cell.
    pub fn centre(&self, cell: usize) -> [f64; 3] {
        let c = self.cell_ijk(cell);
        std::array::from_fn(|d| 0.5 * (self.planes[d][c[d]] + self.planes[d][c[d] + 1]))
    }

    /// The side lengths of a cell.
    pub fn size(&self, cell: usize) -> [f64; 3] {
        let c = self.cell_ijk(cell);
        std::array::from_fn(|d| self.planes[d][c[d] + 1] - self.planes[d][c[d]])
    }

    /// The 8 grid points of a cell, in the order of a VTK hexahedron.
    pub fn corners(&self, cell: usize) -> [[usize; 3]; 8] {
        let [i, j, k] = self.cell_ijk(cell);
        [
            [i, j, k],
            [i + 1, j, k],
            [i + 1, j + 1, k],
            [i, j + 1, k],
            [i, j, k + 1],
            [i + 1, j, k + 1],
            [i + 1, j + 1, k + 1],
            [i, j + 1, k + 1],
        ]
    }

    /// The cell beyond a side of a cell, if the grid has one.
    pub fn neighbour(&self, cell: usize, side: Side) -> Option<usize> {
        let mut c = self.cell_ijk(cell);
        let n = self.dims()[side.axis];
        if side.positive {
            if c[side.axis] + 1 >= n {
                return None;
            }
            c[side.axis] += 1;
        } else {
            if c[side.axis] == 0 {
                return None;
            }
            c[side.axis] -= 1;
        }
        Some(self.cell_index(c))
    }

    /// Each face of the model: a side of a cell with a part and no part beyond it.
    pub fn faces(&self) -> Vec<Face> {
        let mut faces = Vec::new();
        for cell in 0..self.part.len() {
            if self.part[cell] == VOID {
                continue;
            }
            for (s, side) in Side::ALL.into_iter().enumerate() {
                let outside = self
                    .neighbour(cell, side)
                    .is_none_or(|n| self.part[n] == VOID);
                if outside {
                    faces.push(Face {
                        cell,
                        side: s as u8,
                    });
                }
            }
        }
        faces
    }

    /// The centre of a face.
    pub fn face_centre(&self, face: Face) -> [f64; 3] {
        let side = Side::ALL[face.side as usize];
        let c = self.cell_ijk(face.cell);
        let mut x = self.centre(face.cell);
        x[side.axis] = self.planes[side.axis][c[side.axis] + usize::from(side.positive)];
        x
    }

    /// The area of a face.
    pub fn face_area(&self, face: Face) -> f64 {
        let side = Side::ALL[face.side as usize];
        let s = self.size(face.cell);
        (0..3).filter(|&d| d != side.axis).map(|d| s[d]).product()
    }

    /// The 4 grid points of a face.
    pub fn face_points(&self, face: Face) -> [[usize; 3]; 4] {
        let side = Side::ALL[face.side as usize];
        let c = self.cell_ijk(face.cell);
        let (a, b) = match side.axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let mut base = c;
        base[side.axis] += usize::from(side.positive);
        let at = |da: usize, db: usize| {
            let mut p = base;
            p[a] += da;
            p[b] += db;
            p
        };
        [at(0, 0), at(1, 0), at(1, 1), at(0, 1)]
    }

    /// Give each cell the part that holds its centre.
    fn assign(&mut self, parts: &[Part]) -> Result<(), String> {
        let mut best: Vec<Option<usize>> = vec![None; self.part.len()];
        for (index, p) in parts.iter().enumerate() {
            let range: [(usize, usize); 3] = std::array::from_fn(|d| {
                let planes = &self.planes[d];
                let first = planes.partition_point(|&x| x < p.lo[d] - tol(planes));
                let end = planes.partition_point(|&x| x < p.hi[d] - tol(planes));
                (first, end)
            });
            for k in range[2].0..range[2].1 {
                for j in range[1].0..range[1].1 {
                    for i in range[0].0..range[0].1 {
                        let cell = self.cell_index([i, j, k]);
                        match best[cell] {
                            Some(other) if parts[other].priority > p.priority => {}
                            Some(other)
                                if parts[other].priority == p.priority
                                    && parts[other].material != p.material =>
                            {
                                let x = self.centre(cell);
                                return Err(format!(
                                    "parts {} and {} overlap at ({}, {}, {}) with the same \
                                     priority and other materials: give the part that must \
                                     hold the overlap a higher priority",
                                    parts[other].name, p.name, x[0], x[1], x[2]
                                ));
                            }
                            _ => best[cell] = Some(index),
                        }
                    }
                }
            }
        }
        for (cell, b) in best.into_iter().enumerate() {
            self.part[cell] = b.map_or(VOID, |b| b as u32);
        }
        Ok(())
    }

    /// The grid cut at the plane `at` of `axis`: only the cells above the plane stay. The plane
    /// must be a grid plane.
    pub fn crop(&self, axis: usize, at: f64) -> (Grid, Vec<usize>) {
        let p = &self.planes[axis];
        let first = p
            .iter()
            .position(|&x| (x - at).abs() <= tol(p))
            .expect("the cut is a grid plane");
        let mut planes = self.planes.clone();
        planes[axis] = p[first..].to_vec();
        let dims = self.dims();
        let mut part = Vec::new();
        // Item n of `old` is the cell of the full grid of the new cell n.
        let mut old = Vec::new();
        for k in 0..dims[2] {
            for j in 0..dims[1] {
                for i in 0..dims[0] {
                    if [i, j, k][axis] >= first {
                        let cell = self.cell_index([i, j, k]);
                        part.push(self.part[cell]);
                        old.push(cell);
                    }
                }
            }
        }
        (Grid { planes, part }, old)
    }

    /// The cell that mirrors `cell` at the middle plane of `axis`.
    pub fn mirror_cell(&self, cell: usize, axis: usize) -> usize {
        let mut c = self.cell_ijk(cell);
        c[axis] = self.dims()[axis] - 1 - c[axis];
        self.cell_index(c)
    }

    /// True when the planes of `axis` mirror at the plane `at`.
    pub fn planes_mirror(&self, axis: usize, at: f64) -> bool {
        let p = &self.planes[axis];
        let n = p.len();
        (0..n).all(|i| ((p[i] - at) + (p[n - 1 - i] - at)).abs() <= tol(p))
    }

    /// The cell that holds the point `x`, and the local coordinates (0 to 1) of the point in it.
    /// A point on a plane between a cell of a part and a void cell is in the cell of the part.
    pub fn locate(&self, x: [f64; 3]) -> Option<(usize, [f64; 3])> {
        let mut choices: [Vec<usize>; 3] = Default::default();
        for d in 0..3 {
            let p = &self.planes[d];
            let t = tol(p);
            if x[d] < p[0] - t || x[d] > p[p.len() - 1] + t {
                return None;
            }
            let i = p.partition_point(|&v| v <= x[d]).clamp(1, p.len() - 1) - 1;
            choices[d].push(i);
            // On a plane, the cell below the plane is a choice too.
            if i > 0 && (x[d] - p[i]).abs() <= t {
                choices[d].push(i - 1);
            }
            if i + 2 < p.len() && (x[d] - p[i + 1]).abs() <= t {
                choices[d].push(i + 1);
            }
        }
        for &i in &choices[0] {
            for &j in &choices[1] {
                for &k in &choices[2] {
                    let cell = self.cell_index([i, j, k]);
                    if self.part[cell] != VOID {
                        let c = [i, j, k];
                        let local = std::array::from_fn(|d| {
                            let (a, b) = (self.planes[d][c[d]], self.planes[d][c[d] + 1]);
                            ((x[d] - a) / (b - a)).clamp(0.0, 1.0)
                        });
                        return Some((cell, local));
                    }
                }
            }
        }
        None
    }
}

/// The tolerance of a coordinate on the planes `p`: 1e-9 of the length of the grid.
pub fn tol(p: &[f64]) -> f64 {
    1e-9 * (p[p.len() - 1] - p[0]).abs().max(f64::MIN_POSITIVE)
}

/// The graded planes of 1 axis.
fn axis_planes(
    parts: &[Part],
    d: usize,
    spacing: Spacing,
    extra: &[f64],
) -> Result<Vec<f64>, String> {
    let mut keys: Vec<f64> = parts.iter().flat_map(|p| [p.lo[d], p.hi[d]]).collect();
    keys.extend_from_slice(extra);
    keys.sort_by(f64::total_cmp);
    let t = tol(&keys);
    keys.dedup_by(|a, b| (*a - *b).abs() <= t);
    // The target size of each interval: max_cell, and min_cells through each part over it.
    let target: Vec<f64> = keys
        .windows(2)
        .map(|w| {
            parts
                .iter()
                .filter(|p| p.lo[d] <= w[0] + t && p.hi[d] >= w[1] - t)
                .map(|p| (p.hi[d] - p.lo[d]) / spacing.min_cells as f64)
                .fold(spacing.max_cell, f64::min)
        })
        .collect();
    // The size at each key plane: the smaller target of its 2 intervals. A fix lowers it where 2
    // cells side by side break the growth limit.
    let mut at_key: Vec<f64> = (0..keys.len())
        .map(|k| {
            let left = if k > 0 { target[k - 1] } else { f64::INFINITY };
            let right = target.get(k).copied().unwrap_or(f64::INFINITY);
            left.min(right)
        })
        .collect();
    let slope = 0.5 * (spacing.max_growth - 1.0);
    for _ in 0..60 {
        let planes = place(&keys, &target, &at_key, slope);
        let cells: Vec<f64> = planes.windows(2).map(|w| w[1] - w[0]).collect();
        let mut fixed = false;
        for (c, w) in cells.windows(2).enumerate() {
            let (small, large) = if w[0] < w[1] {
                (w[0], w[1])
            } else {
                (w[1], w[0])
            };
            if large > spacing.max_growth * small * (1.0 + 1e-9) {
                // The key plane next to the large cell gets a smaller size.
                let x = planes[c + 1];
                let k = keys.partition_point(|&v| v < x - t).min(keys.len() - 1);
                for k in [k.saturating_sub(1), k, (k + 1).min(keys.len() - 1)] {
                    let near = (keys[k] - x).abs() <= 2.0 * large;
                    if near && at_key[k] > small * 1.0001 {
                        at_key[k] = small;
                        fixed = true;
                    }
                }
            }
        }
        if !fixed {
            return Ok(planes);
        }
    }
    Err(format!(
        "the {} grid planes break the growth limit max_growth = {}: use a larger max_growth or a \
         smaller max_cell",
        ["x", "y", "z"][d],
        spacing.max_growth
    ))
}

/// The planes of an axis from the size function: in interval i the size is the smaller of
/// `target[i]` and `at_key[k] + slope |x - key[k]|` over each key plane k.
fn place(keys: &[f64], target: &[f64], at_key: &[f64], slope: f64) -> Vec<f64> {
    let h = |x: f64, i: usize| {
        keys.iter()
            .zip(at_key)
            .map(|(&k, &s)| s + slope * (x - k).abs())
            .fold(target[i], f64::min)
    };
    let mut planes = vec![keys[0]];
    for i in 0..keys.len() - 1 {
        let (a, b) = (keys[i], keys[i + 1]);
        // The integral of 1 / h on a fine sample of the interval.
        let samples = 400;
        let step = (b - a) / samples as f64;
        let mut phi = vec![0.0; samples + 1];
        for s in 0..samples {
            let (x0, x1) = (a + s as f64 * step, a + (s + 1) as f64 * step);
            phi[s + 1] = phi[s] + 0.5 * step * (1.0 / h(x0, i) + 1.0 / h(x1, i));
        }
        let total = phi[samples];
        let cells = ((total - 1e-6).ceil() as usize).max(1);
        for c in 1..cells {
            let want = total * c as f64 / cells as f64;
            let s = phi.partition_point(|&v| v < want).clamp(1, samples);
            let f = (want - phi[s - 1]) / (phi[s] - phi[s - 1]);
            planes.push(a + (s as f64 - 1.0 + f) * step);
        }
        planes.push(b);
    }
    planes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(name: &str, material: usize, priority: i64, lo: [f64; 3], hi: [f64; 3]) -> Part {
        Part {
            name: name.into(),
            material,
            priority,
            lo,
            hi,
        }
    }

    const SPACING: Spacing = Spacing {
        max_cell: 1.0,
        max_growth: 1.3,
        min_cells: 2,
    };

    fn ratios(p: &[f64]) -> f64 {
        p.windows(3)
            .map(|w| {
                let (a, b) = (w[1] - w[0], w[2] - w[1]);
                a.max(b) / a.min(b)
            })
            .fold(1.0, f64::max)
    }

    #[test]
    fn each_part_face_is_a_plane_and_the_cells_grow_at_most_by_the_limit() {
        // A thin layer on a thick block.
        let parts = [
            part("block", 0, 0, [0.0, 0.0, 0.0], [10.0, 10.0, 5.0]),
            part("layer", 1, 0, [2.0, 3.0, 5.0], [7.0, 6.0, 5.01]),
        ];
        let g = Grid::build(&parts, SPACING, [&[], &[], &[]]).unwrap();
        for p in &parts {
            for d in 0..3 {
                for x in [p.lo[d], p.hi[d]] {
                    assert!(g.planes[d].iter().any(|&v| (v - x).abs() < 1e-12), "{x}");
                }
            }
        }
        for d in 0..3 {
            assert!(
                ratios(&g.planes[d]) <= 1.3 + 1e-9,
                "axis {d}: {}",
                ratios(&g.planes[d])
            );
            let longest = g.planes[d]
                .windows(2)
                .map(|w| w[1] - w[0])
                .fold(0.0, f64::max);
            assert!(longest <= 1.0 + 1e-12);
        }
        // The layer has at least 2 cells through its thickness.
        let z = &g.planes[2];
        assert!(
            z.iter()
                .filter(|&&v| v > 5.0 + 1e-9 && v < 5.01 - 1e-9)
                .count()
                >= 1
        );
    }

    #[test]
    fn random_parts_keep_the_growth_limit() {
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for _ in 0..30 {
            let parts: Vec<Part> = (0..5)
                .map(|i| {
                    let lo: [f64; 3] = std::array::from_fn(|_| 10.0 * next());
                    let hi: [f64; 3] = std::array::from_fn(|d| lo[d] + 0.01 + 5.0 * next().powi(3));
                    part(&format!("p{i}"), 0, 0, lo, hi)
                })
                .collect();
            let g = Grid::build(&parts, SPACING, [&[], &[], &[]]).unwrap();
            for d in 0..3 {
                assert!(
                    ratios(&g.planes[d]) <= 1.3 + 1e-9,
                    "{}",
                    ratios(&g.planes[d])
                );
            }
        }
    }

    #[test]
    fn the_part_with_the_higher_priority_holds_the_overlap() {
        let parts = [
            part("a", 0, 0, [0.0; 3], [2.0; 3]),
            part("b", 1, 1, [1.0; 3], [3.0; 3]),
        ];
        let g = Grid::build(&parts, SPACING, [&[], &[], &[]]).unwrap();
        let (cell, _) = g.locate([1.5, 1.5, 1.5]).unwrap();
        assert_eq!(g.part[cell], 1);
        let (cell, _) = g.locate([0.5, 0.5, 0.5]).unwrap();
        assert_eq!(g.part[cell], 0);
        assert!(
            g.locate([2.5, 0.5, 0.5]).is_none(),
            "a void point is in no cell"
        );
        let same = [
            part("a", 0, 0, [0.0; 3], [2.0; 3]),
            part("b", 1, 0, [1.0; 3], [3.0; 3]),
        ];
        let e = Grid::build(&same, SPACING, [&[], &[], &[]]).unwrap_err();
        assert!(e.contains("same priority"), "{e}");
    }

    #[test]
    fn the_faces_of_a_box_cover_its_surface() {
        let parts = [part("a", 0, 0, [0.0; 3], [2.0, 3.0, 4.0])];
        let g = Grid::build(&parts, SPACING, [&[], &[], &[]]).unwrap();
        let area: f64 = g.faces().iter().map(|&f| g.face_area(f)).sum();
        assert!((area - 2.0 * (6.0 + 8.0 + 12.0)).abs() < 1e-9);
    }
}
