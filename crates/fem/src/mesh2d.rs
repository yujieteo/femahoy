//! A quality triangle mesh of a planar straight line graph: points and segments in the plane,
//! for example the outlines and the holes of polygons.
//!
//! 1. The input is cleaned: points closer than a tolerance merge, and each segment is split at
//!    each point on it and at each crossing with another segment.
//! 2. The points go into a Delaunay triangulation, 1 at a time: each point splits the triangle
//!    (or the edge) that holds it, and edge flips restore the empty circle property. A walk from
//!    the last triangle finds the triangle that holds a point.
//! 3. A segment that is not an edge is split at its middle until each piece is an edge
//!    (recovery by splits). The edges of the segments are then fixed: no flip removes them.
//! 4. The regions that the fixed edges bound get a label from the caller (for example the part
//!    that holds them, or none outside the model).
//! 5. Ruppert refinement: a piece of a segment whose diametral circle holds a vertex
//!    (encroached) is split at its middle. A triangle whose circumradius to shortest edge ratio is
//!    above sqrt(2) (an angle below 20.7 degrees), or whose size is above the size field, gets a
//!    vertex at its circumcentre. A circumcentre that encroaches a piece of a segment is not
//!    put in: the piece is split instead. A piece with 1 end at an input point is split at a
//!    power of 2 distance from it (concentric shells), and a skinny triangle whose short edge
//!    joins 2 segments that meet at an angle below 60 degrees is left, so a small input angle
//!    does not make the refinement go on for ever.
//!
//! The size field is graded: an input point has the length of its shortest segment, and the size
//! grows by the factor `max_growth` - 1 times the distance from each vertex, up to `max_size`.
//!
//! Sources: C. L. Lawson, Software for C1 surface interpolation, in Mathematical Software III,
//! Academic Press 1977 (the flip algorithm); D. F. Watson, Computing the n-dimensional Delaunay
//! tessellation, Computer Journal 24 (1981) 167-172, and A. Bowyer, Computing Dirichlet
//! tessellations, same issue 162-166 (incremental insertion and the cavity of a point);
//! O. Devillers, S. Pion and M. Teillaud, Walking in a triangulation, Int. J. Found. Comput. Sci.
//! 13 (2002) 181-199 (the walk); J. Ruppert, A Delaunay refinement algorithm for quality
//! 2-dimensional mesh generation, J. Algorithms 18 (1995) 548-585; J. R. Shewchuk, Delaunay
//! refinement algorithms for triangular mesh generation, Comput. Geom. 22 (2002) 21-74
//! (concentric shells, the rule for small input angles, and the split of a piece that a
//! circumcentre lies beyond); P. J. Frey and P.-L. George, Mesh Generation, 2nd ed., Wiley 2008,
//! chapters 7 and 20 (constrained triangulations and size maps).

use std::collections::{HashMap, VecDeque};

use crate::predicates::{incircle, orient2d};

const NONE: u32 = u32::MAX;

/// The label of a region outside the model.
pub const OUTSIDE: u32 = u32::MAX;

/// The circumradius to shortest edge ratio of a good triangle: sqrt(2), so each angle is at least
/// asin(1 / (2 sqrt(2))) = 20.7 degrees. Reason: Ruppert refinement ends for a bound of sqrt(2)
/// or more when no 2 segments meet at an angle below 60 degrees (Shewchuk 2002, theorem 7).
pub const RATIO: f64 = std::f64::consts::SQRT_2;

/// The smallest angle, in degrees, that `RATIO` gives.
pub const MIN_ANGLE: f64 = 20.7;

/// A mesh with more vertices than this is refused. Reason: 4,000,000 vertices extruded through
/// even 2 planes give 8,000,000 points, twice the limit of the solver.
const MAX_VERTICES: usize = 4_000_000;

/// The sizes of the triangles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    /// The largest edge length of a triangle (about).
    pub max_size: f64,
    /// The largest size ratio of 2 triangles side by side (about), above 1.
    pub max_growth: f64,
}

/// The mesh of the regions with a label.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh2 {
    pub points: Vec<[f64; 2]>,
    /// The 3 points of each triangle, counterclockwise.
    pub triangles: Vec<[u32; 3]>,
    /// The label of each triangle.
    pub label: Vec<u32>,
    /// The input segments that hold each edge on a segment, by the 2 points of the edge (the
    /// smaller index first).
    pub segments: HashMap<(u32, u32), Vec<u32>>,
}

/// The mesh of the points `points` and the segments `segments` (pairs of indices into
/// `points`). `classify` gives the label of a point inside a region that the segments bound, or
/// `OUTSIDE`. Only the triangles with a label are refined and kept.
pub fn mesh(
    points: &[[f64; 2]],
    segments: &[[u32; 2]],
    size: Size,
    classify: &dyn Fn([f64; 2]) -> u32,
) -> Result<Mesh2, String> {
    if !(size.max_size > 0.0 && size.max_size.is_finite()) {
        return Err("the largest cell size must be a number above 0".into());
    }
    if !(size.max_growth > 1.0 && size.max_growth.is_finite()) {
        return Err("max_growth must be a number above 1".into());
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("each point of an outline must have finite coordinates".into());
    }
    let (pts, pieces) = clean(points, segments);
    if pieces.is_empty() {
        return Err("the outlines have no segment".into());
    }
    let mut t = Triangulation::new(&pts, size)?;
    for p in &pieces {
        let len = dist(pts[p.ends[0] as usize], pts[p.ends[1] as usize]);
        for &v in &p.ends {
            let h = &mut t.size[v as usize];
            *h = h.min(len);
        }
        t.piece_ends.push(p.ends);
        t.piece_origin.push(p.origin.clone());
    }
    t.recover()?;
    t.label_regions(classify);
    t.refine()?;
    Ok(t.output())
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// A segment after the clean: its 2 ends and the input segments that hold it.
struct Piece {
    ends: [u32; 2],
    origin: Vec<u32>,
}

/// Merge the points closer than the tolerance, and split each segment at the points on it and at
/// its crossings with other segments.
fn clean(points: &[[f64; 2]], segments: &[[u32; 2]]) -> (Vec<[f64; 2]>, Vec<Piece>) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in points {
        for d in 0..2 {
            lo[d] = lo[d].min(p[d]);
            hi[d] = hi[d].max(p[d]);
        }
    }
    let tol = 1e-9 * dist(lo, hi).max(f64::MIN_POSITIVE);
    let mut pts: Vec<[f64; 2]> = Vec::new();
    // Merge: a point takes the index of an earlier point within the tolerance.
    let mut order: Vec<usize> = (0..points.len()).collect();
    order.sort_by(|&a, &b| points[a][0].total_cmp(&points[b][0]));
    let mut map = vec![NONE; points.len()];
    for (k, &i) in order.iter().enumerate() {
        if map[i] != NONE {
            continue;
        }
        map[i] = pts.len() as u32;
        for &j in &order[k + 1..] {
            if points[j][0] - points[i][0] > tol {
                break;
            }
            if map[j] == NONE && dist(points[i], points[j]) <= tol {
                map[j] = pts.len() as u32;
            }
        }
        pts.push(points[i]);
    }
    let mut pieces: Vec<Piece> = segments
        .iter()
        .enumerate()
        .map(|(k, s)| Piece {
            ends: [map[s[0] as usize], map[s[1] as usize]],
            origin: vec![k as u32],
        })
        .filter(|p| p.ends[0] != p.ends[1])
        .collect();
    // Split until no segment holds a point inside it or crosses another segment. Each pass splits
    // at all the points that it finds; a new crossing point can lie on a third segment, so a
    // second pass checks.
    for _ in 0..8 {
        let mut cuts: Vec<Vec<u32>> = vec![Vec::new(); pieces.len()];
        let mut changed = false;
        let bbox = |p: &Piece| {
            let (a, b) = (pts[p.ends[0] as usize], pts[p.ends[1] as usize]);
            (
                [a[0].min(b[0]), a[1].min(b[1])],
                [a[0].max(b[0]), a[1].max(b[1])],
            )
        };
        let mut by_x: Vec<usize> = (0..pieces.len()).collect();
        by_x.sort_by(|&a, &b| bbox(&pieces[a]).0[0].total_cmp(&bbox(&pieces[b]).0[0]));
        let mut new_points: Vec<[f64; 2]> = Vec::new();
        for (k, &i) in by_x.iter().enumerate() {
            let (ilo, ihi) = bbox(&pieces[i]);
            for &j in &by_x[k + 1..] {
                let (jlo, jhi) = bbox(&pieces[j]);
                if jlo[0] > ihi[0] + tol {
                    break;
                }
                if jlo[1] > ihi[1] + tol || jhi[1] < ilo[1] - tol {
                    continue;
                }
                let [a, b] = pieces[i].ends;
                let [c, d] = pieces[j].ends;
                // An end of 1 segment inside the other.
                for (s, ends, other) in [(i, [a, b], [c, d]), (j, [c, d], [a, b])] {
                    for &v in &other {
                        if !ends.contains(&v) && on_segment(&pts, ends, v, tol) {
                            cuts[s].push(v);
                            changed = true;
                        }
                    }
                }
                if [a, b].contains(&c) || [a, b].contains(&d) {
                    continue;
                }
                let (pa, pb, pc, pd) = (
                    pts[a as usize],
                    pts[b as usize],
                    pts[c as usize],
                    pts[d as usize],
                );
                let (o1, o2) = (orient2d(pa, pb, pc), orient2d(pa, pb, pd));
                let (o3, o4) = (orient2d(pc, pd, pa), orient2d(pc, pd, pb));
                if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
                    // A proper crossing: the point at the parameter o1 / (o1 - o2) along c-d.
                    let s = o1 / (o1 - o2);
                    let x = [pc[0] + s * (pd[0] - pc[0]), pc[1] + s * (pd[1] - pc[1])];
                    let v = match new_points.iter().position(|q| dist(*q, x) <= tol) {
                        Some(k) => (pts.len() + k) as u32,
                        None => {
                            new_points.push(x);
                            (pts.len() + new_points.len() - 1) as u32
                        }
                    };
                    cuts[i].push(v);
                    cuts[j].push(v);
                    changed = true;
                }
            }
        }
        pts.extend(new_points);
        if !changed {
            break;
        }
        let mut next = Vec::new();
        for (p, mut cut) in pieces.into_iter().zip(cuts) {
            let a = pts[p.ends[0] as usize];
            let b = pts[p.ends[1] as usize];
            let along = |v: u32| {
                let q = pts[v as usize];
                (q[0] - a[0]) * (b[0] - a[0]) + (q[1] - a[1]) * (b[1] - a[1])
            };
            cut.sort_by(|&x, &y| along(x).total_cmp(&along(y)));
            cut.dedup();
            let mut prev = p.ends[0];
            for v in cut.into_iter().chain([p.ends[1]]) {
                if v != prev {
                    next.push(Piece {
                        ends: [prev, v],
                        origin: p.origin.clone(),
                    });
                    prev = v;
                }
            }
        }
        pieces = next;
    }
    // Segments on the same 2 points are 1 piece with the input segments of each.
    let mut merged: HashMap<(u32, u32), usize> = HashMap::new();
    let mut out: Vec<Piece> = Vec::new();
    for p in pieces {
        let key = (p.ends[0].min(p.ends[1]), p.ends[0].max(p.ends[1]));
        match merged.get(&key) {
            Some(&k) => {
                for o in p.origin {
                    if !out[k].origin.contains(&o) {
                        out[k].origin.push(o);
                    }
                }
            }
            None => {
                merged.insert(key, out.len());
                out.push(p);
            }
        }
    }
    (pts, out)
}

/// True when the point `v` is on the segment `ends`, not at an end, within `tol`.
fn on_segment(pts: &[[f64; 2]], ends: [u32; 2], v: u32, tol: f64) -> bool {
    let (a, b, p) = (
        pts[ends[0] as usize],
        pts[ends[1] as usize],
        pts[v as usize],
    );
    let len2 = (b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2);
    let s = ((p[0] - a[0]) * (b[0] - a[0]) + (p[1] - a[1]) * (b[1] - a[1])) / len2;
    if !(s > 0.0 && s < 1.0) {
        return false;
    }
    let foot = [a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1])];
    dist(foot, p) <= tol && dist(p, a) > tol && dist(p, b) > tol
}

/// A triangle: its points counterclockwise, the triangle across the edge opposite each point
/// (`NONE` at the outer hull), and the piece of a segment on that edge (`NONE`: no segment).
#[derive(Debug, Clone, Copy)]
struct Tri {
    v: [u32; 3],
    n: [u32; 3],
    seg: [u32; 3],
    label: u32,
}

/// Where a point is.
enum Locate {
    Inside(usize),
    /// On the edge opposite point `i` of the triangle.
    Edge(usize, usize),
    Vertex(u32),
}

struct Triangulation {
    pts: Vec<[f64; 2]>,
    /// The target size at each point.
    size: Vec<f64>,
    /// The piece of a segment that a point split, or `NONE` (an input point or a free point).
    on_piece: Vec<u32>,
    /// The number of input points (after the clean). The 3 points after them are the corners of
    /// the outer triangle.
    inputs: usize,
    tris: Vec<Tri>,
    /// A triangle of each point.
    at: Vec<u32>,
    piece_ends: Vec<[u32; 2]>,
    piece_origin: Vec<Vec<u32>>,
    max_size: f64,
    growth: f64,
    last: usize,
}

impl Triangulation {
    /// The Delaunay triangulation of `pts` in a large outer triangle.
    fn new(pts: &[[f64; 2]], size: Size) -> Result<Triangulation, String> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in pts {
            for d in 0..2 {
                lo[d] = lo[d].min(p[d]);
                hi[d] = hi[d].max(p[d]);
            }
        }
        let c = [0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1])];
        let r = 100.0 * dist(lo, hi).max(size.max_size);
        let n = pts.len();
        let mut all = pts.to_vec();
        all.push([c[0] - 2.0 * r, c[1] - r]);
        all.push([c[0] + 2.0 * r, c[1] - r]);
        all.push([c[0], c[1] + 2.0 * r]);
        let mut t = Triangulation {
            size: vec![size.max_size; n + 3],
            on_piece: vec![NONE; n + 3],
            inputs: n,
            tris: vec![Tri {
                v: [n as u32, n as u32 + 1, n as u32 + 2],
                n: [NONE; 3],
                seg: [NONE; 3],
                label: OUTSIDE,
            }],
            at: vec![0; n + 3],
            pts: all,
            piece_ends: Vec::new(),
            piece_origin: Vec::new(),
            max_size: size.max_size,
            growth: size.max_growth - 1.0,
            last: 0,
        };
        // Insert in the order of x, so each walk is short.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| {
            pts[a][0]
                .total_cmp(&pts[b][0])
                .then(pts[a][1].total_cmp(&pts[b][1]))
        });
        for i in order {
            let p = t.pts[i];
            match t.locate(p, t.last)? {
                Locate::Inside(tri) => t.split_triangle(tri, i as u32),
                Locate::Edge(tri, e) => t.split_edge(tri, e, i as u32),
                Locate::Vertex(_) => unreachable!("the clean merged equal points"),
            }
        }
        Ok(t)
    }

    fn p(&self, v: u32) -> [f64; 2] {
        self.pts[v as usize]
    }

    /// The local index of point `v` in triangle `t`.
    fn index_of(&self, t: usize, v: u32) -> usize {
        self.tris[t]
            .v
            .iter()
            .position(|&x| x == v)
            .expect("a point of the triangle")
    }

    /// The local index of the edge of `t` across which `other` is.
    fn edge_to(&self, t: usize, other: u32) -> usize {
        self.tris[t]
            .n
            .iter()
            .position(|&x| x == other)
            .expect("a neighbour")
    }

    /// Point the neighbour `n` (if any) across from `old` to `new`.
    fn repoint(&mut self, n: u32, old: u32, new: u32) {
        if n != NONE {
            let e = self.edge_to(n as usize, old);
            self.tris[n as usize].n[e] = new;
        }
    }

    fn set_at(&mut self, t: usize) {
        for v in self.tris[t].v {
            self.at[v as usize] = t as u32;
        }
    }

    /// The triangle (or edge, or point) that holds `p`, by a visibility walk from `start`.
    fn locate(&mut self, p: [f64; 2], start: usize) -> Result<Locate, String> {
        let mut t = start;
        let mut turn = 0usize;
        for _ in 0..(4 * self.tris.len() + 100) {
            let tri = self.tris[t];
            turn = turn.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let first = (turn >> 16) % 3;
            let mut moved = false;
            for k in 0..3 {
                let i = (first + k) % 3;
                let a = self.p(tri.v[(i + 1) % 3]);
                let b = self.p(tri.v[(i + 2) % 3]);
                if orient2d(a, b, p) < 0.0 {
                    if tri.n[i] == NONE {
                        return Err("a point is outside the outer triangle".into());
                    }
                    t = tri.n[i] as usize;
                    moved = true;
                    break;
                }
            }
            if !moved {
                self.last = t;
                let zero: Vec<usize> = (0..3)
                    .filter(|&i| {
                        orient2d(self.p(tri.v[(i + 1) % 3]), self.p(tri.v[(i + 2) % 3]), p) == 0.0
                    })
                    .collect();
                return Ok(match zero.len() {
                    0 => Locate::Inside(t),
                    1 => Locate::Edge(t, zero[0]),
                    _ => Locate::Vertex(
                        *tri.v.iter().find(|&&v| self.p(v) == p).unwrap_or(&tri.v[0]),
                    ),
                });
            }
        }
        Err("the walk to a point did not end".into())
    }

    /// Put point `v` inside triangle `t`, then flip to the Delaunay triangulation.
    fn split_triangle(&mut self, t: usize, v: u32) {
        let old = self.tris[t];
        let [a, b, c] = old.v;
        let (t1, t2) = (self.tris.len() as u32, self.tris.len() as u32 + 1);
        let tu = t as u32;
        self.tris[t] = Tri {
            v: [v, b, c],
            n: [old.n[0], t1, t2],
            seg: [old.seg[0], NONE, NONE],
            label: old.label,
        };
        self.tris.push(Tri {
            v: [v, c, a],
            n: [old.n[1], t2, tu],
            seg: [old.seg[1], NONE, NONE],
            label: old.label,
        });
        self.tris.push(Tri {
            v: [v, a, b],
            n: [old.n[2], tu, t1],
            seg: [old.seg[2], NONE, NONE],
            label: old.label,
        });
        self.repoint(old.n[1], tu, t1);
        self.repoint(old.n[2], tu, t2);
        for x in [t, t1 as usize, t2 as usize] {
            self.set_at(x);
        }
        self.legalize(vec![(t, 0), (t1 as usize, 0), (t2 as usize, 0)]);
    }

    /// Put point `v` on the edge opposite point `i` of triangle `t`. On a segment, both halves
    /// keep the segment.
    fn split_edge(&mut self, t: usize, i: usize, v: u32) {
        let old = self.tris[t];
        let a = old.v[i];
        let b = old.v[(i + 1) % 3];
        let c = old.v[(i + 2) % 3];
        let seg = old.seg[i];
        let (t_a, s_a) = (old.n[(i + 1) % 3], old.seg[(i + 1) % 3]); // edge c-a
        let (t_b, s_b) = (old.n[(i + 2) % 3], old.seg[(i + 2) % 3]); // edge a-b
        let n = old.n[i];
        let t3 = self.tris.len() as u32;
        let tu = t as u32;
        if n == NONE {
            self.tris[t] = Tri {
                v: [a, b, v],
                n: [NONE, t3, t_b],
                seg: [seg, NONE, s_b],
                label: old.label,
            };
            self.tris.push(Tri {
                v: [a, v, c],
                n: [NONE, t_a, tu],
                seg: [seg, s_a, NONE],
                label: old.label,
            });
            self.repoint(t_a, tu, t3);
            self.set_at(t);
            self.set_at(t3 as usize);
            self.legalize(vec![(t, 2), (t3 as usize, 1)]);
            return;
        }
        let nu = n as usize;
        let on = self.tris[nu];
        let j = self.edge_to(nu, tu);
        let d = on.v[j];
        let (n_d, sn_d) = (on.n[(j + 2) % 3], on.seg[(j + 2) % 3]); // edge d-c
        let (n_c, sn_c) = (on.n[(j + 1) % 3], on.seg[(j + 1) % 3]); // edge b-d
        let t4 = t3 + 1;
        self.tris[t] = Tri {
            v: [a, b, v],
            n: [t4, t3, t_b],
            seg: [seg, NONE, s_b],
            label: old.label,
        };
        self.tris.push(Tri {
            v: [a, v, c],
            n: [n, t_a, tu],
            seg: [seg, s_a, NONE],
            label: old.label,
        });
        self.tris[nu] = Tri {
            v: [d, c, v],
            n: [t3, t4, n_d],
            seg: [seg, NONE, sn_d],
            label: on.label,
        };
        self.tris.push(Tri {
            v: [d, v, b],
            n: [tu, n_c, n],
            seg: [seg, sn_c, NONE],
            label: on.label,
        });
        self.repoint(t_a, tu, t3);
        self.repoint(n_c, n, t4);
        for x in [t, t3 as usize, nu, t4 as usize] {
            self.set_at(x);
        }
        self.legalize(vec![(t, 2), (t3 as usize, 1), (nu, 2), (t4 as usize, 1)]);
    }

    /// Flip each edge in `stack` (the edge opposite point `i` of triangle `t`, where that point is
    /// the new point) whose other triangle has its far point in the circumcircle.
    fn legalize(&mut self, mut stack: Vec<(usize, usize)>) {
        while let Some((t, i)) = stack.pop() {
            let tri = self.tris[t];
            let n = tri.n[i];
            if n == NONE || tri.seg[i] != NONE {
                continue;
            }
            let j = self.edge_to(n as usize, t as u32);
            let d = self.tris[n as usize].v[j];
            let [a, b, c] = tri.v.map(|v| self.p(v));
            if incircle(a, b, c, self.p(d)) > 0.0 {
                self.flip(t, i);
                stack.push((t, 0));
                stack.push((n as usize, 0));
            }
        }
    }

    /// Flip the edge opposite point `i` of `t`. Point `i` (a) stays at index 0 of both new
    /// triangles: t becomes (a, b, d) and its neighbour becomes (a, d, c).
    fn flip(&mut self, t: usize, i: usize) {
        let tri = self.tris[t];
        let a = tri.v[i];
        let b = tri.v[(i + 1) % 3];
        let c = tri.v[(i + 2) % 3];
        let (t_a, s_a) = (tri.n[(i + 1) % 3], tri.seg[(i + 1) % 3]); // edge c-a
        let (t_b, s_b) = (tri.n[(i + 2) % 3], tri.seg[(i + 2) % 3]); // edge a-b
        let n = tri.n[i] as usize;
        let on = self.tris[n];
        let j = self.edge_to(n, t as u32);
        let d = on.v[j];
        let (n_c, sn_c) = (on.n[(j + 1) % 3], on.seg[(j + 1) % 3]); // edge b-d
        let (n_d, sn_d) = (on.n[(j + 2) % 3], on.seg[(j + 2) % 3]); // edge d-c
        self.tris[t] = Tri {
            v: [a, b, d],
            n: [n_c, n as u32, t_b],
            seg: [sn_c, NONE, s_b],
            label: tri.label,
        };
        self.tris[n] = Tri {
            v: [a, d, c],
            n: [n_d, t_a, t as u32],
            seg: [sn_d, s_a, NONE],
            label: on.label,
        };
        self.repoint(n_c, n as u32, t as u32);
        self.repoint(t_a, t as u32, n as u32);
        self.set_at(t);
        self.set_at(n);
    }

    /// The triangle and the local index of the edge a-b, if the triangulation has it.
    fn find_edge(&self, a: u32, b: u32) -> Option<(usize, usize)> {
        let start = self.at[a as usize] as usize;
        // Turn around a in both directions, in case a is on the hull.
        for dir in [1usize, 2] {
            let mut t = start;
            for _ in 0..10_000 {
                let k = self.index_of(t, a);
                let tri = self.tris[t];
                if tri.v[(k + 1) % 3] == b {
                    return Some((t, (k + 2) % 3));
                }
                if tri.v[(k + 2) % 3] == b {
                    return Some((t, (k + 1) % 3));
                }
                let next = tri.n[(k + dir) % 3];
                if next == NONE || next as usize == start {
                    break;
                }
                t = next as usize;
            }
        }
        None
    }

    /// The point that splits the piece a-b of the segment `piece`: the middle, or a power of 2
    /// distance from an input point at 1 end (concentric shells).
    fn split_point(&self, a: u32, b: u32) -> [f64; 2] {
        let (pa, pb) = (self.p(a), self.p(b));
        let len = dist(pa, pb);
        let input = |v: u32| (v as usize) < self.inputs;
        let from = match (input(a), input(b)) {
            (true, false) => Some((pa, pb)),
            (false, true) => Some((pb, pa)),
            _ => None,
        };
        match from {
            Some((p, q)) => {
                let d = 2f64.powf((0.5 * len).log2().round());
                let s = (d / len).clamp(0.25, 0.75);
                [p[0] + s * (q[0] - p[0]), p[1] + s * (q[1] - p[1])]
            }
            None => [0.5 * (pa[0] + pb[0]), 0.5 * (pa[1] + pb[1])],
        }
    }

    /// The size at a new point `p` from the points `near`.
    fn size_at(&self, p: [f64; 2], near: &[u32]) -> f64 {
        near.iter()
            .map(|&v| self.size[v as usize] + self.growth * dist(p, self.p(v)))
            .fold(self.max_size, f64::min)
    }

    /// Add a point, and return its index.
    fn add_point(&mut self, p: [f64; 2], size: f64, piece: u32) -> Result<u32, String> {
        if self.pts.len() >= MAX_VERTICES {
            return Err(format!(
                "the 2D mesh needs more than {MAX_VERTICES} points: use a larger max_cell, or \
                 remove a very small detail of an outline"
            ));
        }
        self.pts.push(p);
        self.size.push(size);
        self.on_piece.push(piece);
        self.at.push(0);
        Ok(self.pts.len() as u32 - 1)
    }

    /// Split the piece a-b of segment `piece` (an edge of the triangulation) at its split point.
    fn split_piece(&mut self, a: u32, b: u32, piece: u32) -> Result<u32, String> {
        let (t, i) = self
            .find_edge(a, b)
            .expect("a piece of a segment is an edge");
        let m = self.split_point(a, b);
        let h = self.size_at(m, &[a, b]);
        let v = self.add_point(m, h, piece)?;
        self.split_edge(t, i, v);
        Ok(v)
    }

    /// Make each segment a chain of edges: split a segment that is not an edge at its split
    /// point. Then fix the edges of the segments.
    fn recover(&mut self) -> Result<(), String> {
        let mut queue: Vec<(u32, u32, u32)> = self
            .piece_ends
            .iter()
            .enumerate()
            .map(|(k, e)| (e[0], e[1], k as u32))
            .collect();
        while let Some((a, b, piece)) = queue.pop() {
            if let Some((t, i)) = self.find_edge(a, b) {
                self.tris[t].seg[i] = piece;
                let n = self.tris[t].n[i];
                if n != NONE {
                    let j = self.edge_to(n as usize, t as u32);
                    self.tris[n as usize].seg[j] = piece;
                }
                continue;
            }
            let m = self.split_point(a, b);
            let start = self.at[a as usize] as usize;
            let v = match self.locate(m, start)? {
                Locate::Vertex(v) => v,
                Locate::Inside(t) => {
                    let v = self.add_point(m, self.size_at(m, &[a, b]), piece)?;
                    self.split_triangle(t, v);
                    v
                }
                Locate::Edge(t, i) => {
                    let v = self.add_point(m, self.size_at(m, &[a, b]), piece)?;
                    self.split_edge(t, i, v);
                    v
                }
            };
            if v == a || v == b {
                return Err("2 segments of the outlines are too close to mesh".into());
            }
            queue.push((a, v, piece));
            queue.push((v, b, piece));
        }
        Ok(())
    }

    /// Give each region that the segments bound the label of a point inside it. A region that
    /// touches the outer triangle is outside.
    fn label_regions(&mut self, classify: &dyn Fn([f64; 2]) -> u32) {
        let mut region = vec![NONE; self.tris.len()];
        for start in 0..self.tris.len() {
            if region[start] != NONE {
                continue;
            }
            let mut members = vec![start];
            region[start] = start as u32;
            let mut k = 0;
            while k < members.len() {
                let tri = self.tris[members[k]];
                for i in 0..3 {
                    let n = tri.n[i];
                    if n != NONE && tri.seg[i] == NONE && region[n as usize] == NONE {
                        region[n as usize] = start as u32;
                        members.push(n as usize);
                    }
                }
                k += 1;
            }
            let outer = members.iter().any(|&t| {
                self.tris[t]
                    .v
                    .iter()
                    .any(|&v| (v as usize) >= self.inputs && (v as usize) < self.inputs + 3)
            });
            let label = if outer {
                OUTSIDE
            } else {
                // The centroid of the largest triangle: well inside the region.
                let area = |t: usize| {
                    let [a, b, c] = self.tris[t].v.map(|v| self.p(v));
                    orient2d(a, b, c)
                };
                let big = *members
                    .iter()
                    .max_by(|&&x, &&y| area(x).total_cmp(&area(y)))
                    .expect("a region has a triangle");
                let [a, b, c] = self.tris[big].v.map(|v| self.p(v));
                classify([(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
            };
            for t in members {
                self.tris[t].label = label;
            }
        }
    }

    /// The circumcentre and the squared circumradius of triangle `t`.
    fn circumcentre(&self, t: usize) -> ([f64; 2], f64) {
        let [a, b, c] = self.tris[t].v.map(|v| self.p(v));
        let (bx, by) = (b[0] - a[0], b[1] - a[1]);
        let (cx, cy) = (c[0] - a[0], c[1] - a[1]);
        let d = 2.0 * orient2d([0.0, 0.0], [bx, by], [cx, cy]);
        let b2 = bx * bx + by * by;
        let c2 = cx * cx + cy * cy;
        let ux = (cy * b2 - by * c2) / d;
        let uy = (bx * c2 - cx * b2) / d;
        ([a[0] + ux, a[1] + uy], ux * ux + uy * uy)
    }

    /// True when triangle `t` (with a label) is skinny or too large, and the rule for small input
    /// angles does not keep it.
    fn bad(&self, t: usize) -> bool {
        let tri = self.tris[t];
        if tri.label == OUTSIDE {
            return false;
        }
        let p = tri.v.map(|v| self.p(v));
        let len2: [f64; 3] = std::array::from_fn(|i| {
            let (a, b) = (p[(i + 1) % 3], p[(i + 2) % 3]);
            (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
        });
        let short = (0..3)
            .min_by(|&x, &y| len2[x].total_cmp(&len2[y]))
            .expect("3 edges");
        let (_, r2) = self.circumcentre(t);
        let centroid = [
            (p[0][0] + p[1][0] + p[2][0]) / 3.0,
            (p[0][1] + p[1][1] + p[2][1]) / 3.0,
        ];
        let h = self.size_at(centroid, &tri.v);
        if 3.0 * r2 > h * h {
            return true;
        }
        if r2 <= RATIO * RATIO * len2[short] {
            return false;
        }
        // Skinny. Keep it when its short edge joins 2 segments that meet at a small angle.
        let (u, w) = (tri.v[(short + 1) % 3], tri.v[(short + 2) % 3]);
        !self.small_angle_between(u, w)
    }

    /// True when points u and w are on 2 segments that share an input point where they meet at an
    /// angle below 60 degrees.
    fn small_angle_between(&self, u: u32, w: u32) -> bool {
        let (pu, pw) = (self.on_piece[u as usize], self.on_piece[w as usize]);
        if pu == NONE || pw == NONE || pu == pw {
            return false;
        }
        let (eu, ew) = (self.piece_ends[pu as usize], self.piece_ends[pw as usize]);
        for &z in &eu {
            if ew.contains(&z) {
                let pz = self.p(z);
                let far = |e: [u32; 2]| self.p(if e[0] == z { e[1] } else { e[0] });
                let (x, y) = (far(eu), far(ew));
                let (vx, vy) = ([x[0] - pz[0], x[1] - pz[1]], [y[0] - pz[0], y[1] - pz[1]]);
                let cos =
                    (vx[0] * vy[0] + vx[1] * vy[1]) / ((vx[0].hypot(vx[1])) * (vy[0].hypot(vy[1])));
                return cos > 0.5;
            }
        }
        false
    }

    /// The piece a-b of a segment is encroached when the far point of a triangle beside it, in a
    /// region with a label, is inside its diametral circle.
    fn encroached(&self, t: usize, i: usize) -> bool {
        let check = |t: usize, i: usize| {
            let tri = self.tris[t];
            if tri.label == OUTSIDE {
                return false;
            }
            let (a, b) = (self.p(tri.v[(i + 1) % 3]), self.p(tri.v[(i + 2) % 3]));
            let c = self.p(tri.v[i]);
            (a[0] - c[0]) * (b[0] - c[0]) + (a[1] - c[1]) * (b[1] - c[1]) < 0.0
        };
        if check(t, i) {
            return true;
        }
        let n = self.tris[t].n[i];
        n != NONE && check(n as usize, self.edge_to(n as usize, t as u32))
    }

    /// The triangle that holds `c`, by a straight walk from the centroid of `t`, or the edge of a
    /// segment that the walk meets first.
    fn walk(&self, t: usize, c: [f64; 2]) -> Result<Result<Locate, (usize, usize)>, String> {
        let p = self.tris[t].v.map(|v| self.p(v));
        let s = [
            (p[0][0] + p[1][0] + p[2][0]) / 3.0,
            (p[0][1] + p[1][1] + p[2][1]) / 3.0,
        ];
        let mut t = t;
        let mut from = NONE;
        for _ in 0..(self.tris.len() + 10) {
            let tri = self.tris[t];
            let o: [f64; 3] = std::array::from_fn(|i| {
                orient2d(self.p(tri.v[(i + 1) % 3]), self.p(tri.v[(i + 2) % 3]), c)
            });
            if o.iter().all(|&x| x >= 0.0) {
                let zero: Vec<usize> = (0..3).filter(|&i| o[i] == 0.0).collect();
                return Ok(Ok(match zero.len() {
                    0 => Locate::Inside(t),
                    1 => Locate::Edge(t, zero[0]),
                    _ => Locate::Vertex(tri.v[3 - zero[0] - zero[1]]),
                }));
            }
            // The exit edge: c is beyond it, and the line s-c passes between its 2 points.
            let exit = (0..3)
                .filter(|&i| o[i] < 0.0 && tri.n[i] != from)
                .find(|&i| {
                    let a = orient2d(s, c, self.p(tri.v[(i + 1) % 3]));
                    let b = orient2d(s, c, self.p(tri.v[(i + 2) % 3]));
                    a * b <= 0.0
                })
                .or_else(|| (0..3).find(|&i| o[i] < 0.0))
                .expect("an edge with c beyond it");
            if tri.seg[exit] != NONE {
                return Ok(Err((t, exit)));
            }
            if tri.n[exit] == NONE {
                return Err("a circumcentre is outside the outer triangle".into());
            }
            from = t as u32;
            t = tri.n[exit] as usize;
        }
        Err("the walk to a circumcentre did not end".into())
    }

    /// The pieces of segments on the edges of the cavity of `c` (the triangles whose circumcircle
    /// holds it, from `t`) whose diametral circle holds `c`.
    fn encroached_by(&self, t: usize, c: [f64; 2]) -> Vec<(u32, u32, u32)> {
        let mut seen = vec![t];
        let mut k = 0;
        let mut out = Vec::new();
        while k < seen.len() {
            let tri = self.tris[seen[k]];
            k += 1;
            for i in 0..3 {
                let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                if tri.seg[i] != NONE {
                    let (pa, pb) = (self.p(a), self.p(b));
                    if (pa[0] - c[0]) * (pb[0] - c[0]) + (pa[1] - c[1]) * (pb[1] - c[1]) < 0.0 {
                        out.push((a, b, tri.seg[i]));
                    }
                    continue;
                }
                let n = tri.n[i];
                if n == NONE || seen.contains(&(n as usize)) {
                    continue;
                }
                let [p0, p1, p2] = self.tris[n as usize].v.map(|v| self.p(v));
                if incircle(p0, p1, p2, c) > 0.0 {
                    seen.push(n as usize);
                }
            }
        }
        out
    }

    /// The triangles around point `v`.
    fn star(&self, v: u32) -> Vec<usize> {
        let start = self.at[v as usize] as usize;
        let mut out = vec![start];
        // Turn 1 way; at the hull, turn the other way from the start too.
        for dir in [1usize, 2] {
            let mut t = start;
            loop {
                let k = self.index_of(t, v);
                let next = self.tris[t].n[(k + dir) % 3];
                if next == NONE {
                    break;
                }
                if next as usize == start {
                    return out;
                }
                t = next as usize;
                out.push(t);
            }
        }
        out
    }

    /// Ruppert refinement of the regions with a label.
    fn refine(&mut self) -> Result<(), String> {
        // A piece to split, and true to split it even when no point encroaches it (a circumcentre
        // beyond it or in its diametral circle).
        let mut pieces: VecDeque<(u32, u32, u32, bool)> = VecDeque::new();
        let mut bad: VecDeque<(usize, [u32; 3])> = VecDeque::new();
        for t in 0..self.tris.len() {
            for i in 0..3 {
                let tri = self.tris[t];
                let (a, b) = (tri.v[(i + 1) % 3], tri.v[(i + 2) % 3]);
                if tri.seg[i] != NONE && a < b && self.encroached(t, i) {
                    pieces.push_back((a, b, tri.seg[i], false));
                }
            }
            if self.bad(t) {
                bad.push_back((t, self.tris[t].v));
            }
        }
        loop {
            if let Some((a, b, piece, force)) = pieces.pop_front() {
                let Some((t, i)) = self.find_edge(a, b) else {
                    continue;
                };
                if self.tris[t].seg[i] == NONE || !(force || self.encroached(t, i)) {
                    continue;
                }
                let v = self.split_piece(a, b, piece)?;
                self.after_insert(v, &mut pieces, &mut bad);
                continue;
            }
            let Some((t, v)) = bad.pop_front() else {
                break;
            };
            if self.tris[t].v != v || !self.bad(t) {
                continue;
            }
            let (c, _) = self.circumcentre(t);
            let at = match self.walk(t, c)? {
                Err((s, i)) => {
                    let tri = self.tris[s];
                    pieces.push_back((tri.v[(i + 1) % 3], tri.v[(i + 2) % 3], tri.seg[i], true));
                    bad.push_back((t, v));
                    continue;
                }
                Ok(Locate::Vertex(_)) => continue,
                Ok(at) => at,
            };
            let cavity_start = match at {
                Locate::Inside(s) | Locate::Edge(s, _) => s,
                Locate::Vertex(_) => unreachable!("handled above"),
            };
            if let Locate::Edge(s, i) = at {
                if self.tris[s].seg[i] != NONE {
                    let tri = self.tris[s];
                    pieces.push_back((tri.v[(i + 1) % 3], tri.v[(i + 2) % 3], tri.seg[i], true));
                    bad.push_back((t, v));
                    continue;
                }
            }
            let hit = self.encroached_by(cavity_start, c);
            if !hit.is_empty() {
                pieces.extend(hit.into_iter().map(|(a, b, s)| (a, b, s, true)));
                bad.push_back((t, v));
                continue;
            }
            let h = self.size_at(c, &v);
            let new = self.add_point(c, h, NONE)?;
            match at {
                Locate::Inside(s) => self.split_triangle(s, new),
                Locate::Edge(s, i) => self.split_edge(s, i, new),
                Locate::Vertex(_) => unreachable!("handled above"),
            }
            self.after_insert(new, &mut pieces, &mut bad);
        }
        Ok(())
    }

    /// Queue the bad triangles and the encroached pieces around the new point `v`.
    fn after_insert(
        &mut self,
        v: u32,
        pieces: &mut VecDeque<(u32, u32, u32, bool)>,
        bad: &mut VecDeque<(usize, [u32; 3])>,
    ) {
        for t in self.star(v) {
            let tri = self.tris[t];
            if self.bad(t) {
                bad.push_back((t, tri.v));
            }
            for i in 0..3 {
                if tri.seg[i] != NONE && self.encroached(t, i) {
                    pieces.push_back((tri.v[(i + 1) % 3], tri.v[(i + 2) % 3], tri.seg[i], false));
                }
            }
        }
    }

    /// The triangles with a label, on the points that they use.
    fn output(&self) -> Mesh2 {
        let mut map = vec![NONE; self.pts.len()];
        let mut points = Vec::new();
        let mut triangles = Vec::new();
        let mut label = Vec::new();
        let mut segments = HashMap::new();
        for tri in &self.tris {
            if tri.label == OUTSIDE {
                continue;
            }
            let v = tri.v.map(|v| {
                if map[v as usize] == NONE {
                    map[v as usize] = points.len() as u32;
                    points.push(self.pts[v as usize]);
                }
                map[v as usize]
            });
            for i in 0..3 {
                if tri.seg[i] != NONE {
                    let (a, b) = (v[(i + 1) % 3], v[(i + 2) % 3]);
                    segments.insert(
                        (a.min(b), a.max(b)),
                        self.piece_origin[tri.seg[i] as usize].clone(),
                    );
                }
            }
            triangles.push(v);
            label.push(tri.label);
        }
        Mesh2 {
            points,
            triangles,
            label,
            segments,
        }
    }
}
