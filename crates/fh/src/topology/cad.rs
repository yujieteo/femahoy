//! The CAD of the last design of a topology run: the solid of the material, as a gmsh script and
//! a STEP file, and a check of the solid with `ccx`.
//!
//! The CAD needs a mesh of plane elements (`CPS` or `CPE`). [`write`] makes it in
//! `<run-dir>/topology/cad`:
//!
//! 1. The density of a node is the mean of the densities of its elements. In an element, the
//!    density is linear between the nodes: a quadrilateral is 4 triangles about its centre. The
//!    material is where the density is 0.5 or more.
//! 2. The boundary of the material is a set of closed loops. On the edge of the mesh, a loop has
//!    straight lines. In the mesh, a loop is a curve: 10 steps of Taubin smoothing, then a
//!    `Spline` of gmsh through points 1 element size apart.
//! 3. `design.geo` holds the plane surfaces of the material and their extrusion by the thickness
//!    of the elements (`thickness` of `topology.toml`, else 1). gmsh (see
//!    [`crate::msh::find_gmsh`]) writes the solid to `design.step` in mm. With the units SI-m,
//!    the STEP file has the coordinates times 1000.
//! 4. The check: gmsh meshes the plane surfaces with 6-node triangles of half the element size,
//!    and `ccx` solves `job.inp` in the folder `solve`. `job.inp` has this mesh, the material of
//!    the density 1, and the supports and the loads of the case. A node of the new mesh gets the
//!    supports of a node of the case at the same place. A node on an edge of the case on the edge
//!    of the mesh also gets the supports that both ends of that edge have. A load goes to the node
//!    at the same place. `check.txt` holds the compliance (2 times the strain energy) and the
//!    volume fraction of the solid. If the supports do not hold each part of the mesh, the run
//!    has no check: ccx gives no error for a part that can move.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use fh_core::inp::{Model, Target};

use super::{element_values, keyword, Settings};
use crate::ccx::{supervise, CancelToken, Ccx, Program};
use crate::getdp::failure;

/// The folder of the CAD in the folder of the loop.
const DIR: &str = "cad";
/// The table of the check in [`DIR`]: the compliance and the volume fraction of the solid.
const CHECK: &str = "check.txt";
const STEP: &str = "design.step";
/// The material is where the density is this or more.
const LEVEL: f64 = 0.5;
/// A density nearer to [`LEVEL`] than this is [`LEVEL`], so that no point of the boundary is
/// very near a node.
const SNAP: f64 = 1e-3;
/// The steps of Taubin smoothing, and its 2 factors.
const SMOOTHING: usize = 10;
const FACTORS: [f64; 2] = [0.5, -0.53];
/// The keywords that the check takes in `job.inp` of the case.
const TAKES: [&str; 14] = [
    "*HEADING",
    "*NODE",
    "*ELEMENT",
    "*NSET",
    "*ELSET",
    "*BOUNDARY",
    "*STEP",
    "*STATIC",
    "*CLOAD",
    "*NODEPRINT",
    "*ELPRINT",
    "*NODEFILE",
    "*ELFILE",
    "*ENDSTEP",
];

/// The CAD of a design.
#[derive(Debug, Clone, PartialEq)]
pub struct Cad {
    /// `design.step`.
    pub step: PathBuf,
    /// The thickness of the solid.
    pub thickness: f64,
    /// The check of the solid with `ccx`, or why the run has no check.
    pub check: Result<Check, String>,
}

/// The compliance and the volume fraction of the solid of the CAD, from the check with `ccx`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Check {
    pub compliance: f64,
    pub volume: f64,
}

/// A corner of the triangles: a node of the mesh, or the centre of a quadrilateral (its index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Corner {
    Node(u32),
    Centre(usize),
}

/// A point of the boundary of the material: a corner, or the point of [`LEVEL`] between 2
/// corners, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key {
    At(Corner),
    Cut(Corner, Corner),
}

/// A mesh of plane elements.
struct Plane {
    xy: HashMap<u32, [f64; 2]>,
    /// The corner nodes of each element, counter-clockwise.
    corners: Vec<Vec<u32>>,
    /// The edges on the edge of the mesh, as (smaller node, larger node).
    edges: HashSet<(u32, u32)>,
    /// The edges on the edge of the mesh at each node.
    at_node: HashMap<u32, Vec<(u32, u32)>>,
    /// The square root of the mean area of an element.
    size: f64,
    /// `CPE` or `CPS`.
    family: String,
}

fn area(points: &[[f64; 2]]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

fn plane(model: &Model) -> Result<Plane, String> {
    let xy: HashMap<u32, [f64; 2]> = (model.nodes.iter())
        .map(|n| (n.id, [n.xyz[0], n.xyz[1]]))
        .collect();
    let mut family: Option<String> = None;
    let mut corners = Vec::new();
    let mut total = 0.0;
    for e in &model.elements {
        let t = e.element_type.to_ascii_uppercase();
        let count = match (t.get(..3), t.get(3..4)) {
            (Some("CPE" | "CPS"), Some("3" | "6")) => 3,
            (Some("CPE" | "CPS"), Some("4" | "8")) => 4,
            _ => {
                return Err(format!(
                    "the CAD needs a mesh of plane elements (CPS or CPE), and element {} is {}",
                    e.id, e.element_type
                ))
            }
        };
        if *family.get_or_insert_with(|| t[..3].to_owned()) != t[..3] {
            return Err("the CAD needs 1 family of plane elements: CPS or CPE".into());
        }
        let mut c = e.nodes[..count.min(e.nodes.len())].to_vec();
        let a = area(&c.iter().map(|n| xy[n]).collect::<Vec<_>>());
        if a < 0.0 {
            c.reverse();
        }
        total += a.abs();
        corners.push(c);
    }
    let mut count: HashMap<(u32, u32), usize> = HashMap::new();
    for c in &corners {
        for (i, a) in c.iter().enumerate() {
            let b = c[(i + 1) % c.len()];
            *count.entry((*a.min(&b), *a.max(&b))).or_default() += 1;
        }
    }
    let edges: HashSet<(u32, u32)> = (count.into_iter().filter(|(_, n)| *n == 1))
        .map(|(e, _)| e)
        .collect();
    let mut at_node: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
    for e in &edges {
        at_node.entry(e.0).or_default().push(*e);
        at_node.entry(e.1).or_default().push(*e);
    }
    Ok(Plane {
        xy,
        size: (total / corners.len() as f64).sqrt(),
        corners,
        edges,
        at_node,
        family: family.unwrap_or_default(),
    })
}

/// A corner of a triangle: its key, its place and its density.
type Point = (Corner, [f64; 2], f64);

/// The part of the triangle `t` where the density is [`LEVEL`] or more, as its corners in
/// order. `at` gets the place of each corner.
fn clip(t: &[Point; 3], at: &mut HashMap<Key, [f64; 2]>) -> Vec<Key> {
    let mut out: Vec<Key> = Vec::new();
    let push = |key: Key, out: &mut Vec<Key>| {
        if out.last() != Some(&key) {
            out.push(key);
        }
    };
    for i in 0..3 {
        let (p, q) = (t[i], t[(i + 1) % 3]);
        if p.2 >= LEVEL {
            at.insert(Key::At(p.0), p.1);
            push(Key::At(p.0), &mut out);
        }
        if (p.2 >= LEVEL) != (q.2 >= LEVEL) {
            // The same order on each side of a line gives the same point.
            let (a, b) = if p.0 < q.0 { (p, q) } else { (q, p) };
            let key = if a.2 == LEVEL {
                Key::At(a.0)
            } else if b.2 == LEVEL {
                Key::At(b.0)
            } else {
                let s = (LEVEL - a.2) / (b.2 - a.2);
                let key = Key::Cut(a.0, b.0);
                let place = [
                    a.1[0] + s * (b.1[0] - a.1[0]),
                    a.1[1] + s * (b.1[1] - a.1[1]),
                ];
                at.insert(key, place);
                key
            };
            push(key, &mut out);
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// A closed loop of the boundary of the material, counter-clockwise about the material.
type Loop = Vec<(Key, [f64; 2])>;

/// The boundary of the material of the densities `x` of the elements of `plane`.
fn outline(plane: &Plane, x: &[f64]) -> Result<Vec<Loop>, String> {
    let snap = |v: f64| if (v - LEVEL).abs() < SNAP { LEVEL } else { v };
    let mut sum: HashMap<u32, (f64, f64)> = HashMap::new();
    for (c, x) in plane.corners.iter().zip(x) {
        for n in c {
            let s = sum.entry(*n).or_default();
            *s = (s.0 + x, s.1 + 1.0);
        }
    }
    let node: HashMap<u32, f64> = sum.into_iter().map(|(n, s)| (n, snap(s.0 / s.1))).collect();
    let mut at = HashMap::new();
    let mut sides = HashSet::new();
    for (e, c) in plane.corners.iter().enumerate() {
        let p: Vec<Point> = c
            .iter()
            .map(|n| (Corner::Node(*n), plane.xy[n], node[n]))
            .collect();
        let triangles = if p.len() == 3 {
            vec![[p[0], p[1], p[2]]]
        } else {
            let mean = |f: &dyn Fn(&Point) -> f64| p.iter().map(f).sum::<f64>() / 4.0;
            let centre = (
                Corner::Centre(e),
                [mean(&|p| p.1[0]), mean(&|p| p.1[1])],
                snap(mean(&|p| p.2)),
            );
            (0..4).map(|i| [p[i], p[(i + 1) % 4], centre]).collect()
        };
        for t in &triangles {
            let piece = clip(t, &mut at);
            if piece.len() > 2 {
                for (i, a) in piece.iter().enumerate() {
                    sides.insert((*a, piece[(i + 1) % piece.len()]));
                }
            }
        }
    }
    // A side of 2 parts is in the material. The other sides are the boundary.
    let mut boundary: Vec<(Key, Key)> = (sides.iter())
        .filter(|(a, b)| !sides.contains(&(*b, *a)))
        .copied()
        .collect();
    boundary.sort_unstable();
    let mut from: HashMap<Key, Vec<Key>> = HashMap::new();
    for (a, b) in &boundary {
        from.entry(*a).or_default().push(*b);
    }
    let angle = |a: &Key, b: &Key| (at[b][1] - at[a][1]).atan2(at[b][0] - at[a][0]);
    // Where 2 parts of the material touch at a point, the loop keeps to its part: it takes the
    // first side clockwise from the side back.
    let next = |(u, v): (Key, Key)| -> Option<(Key, Key)> {
        let back = angle(&v, &u);
        let turn = |w: &Key| (back - angle(&v, w)).rem_euclid(std::f64::consts::TAU);
        let w = (from.get(&v)?.iter()).min_by(|a, b| turn(a).total_cmp(&turn(b)))?;
        Some((v, *w))
    };
    let open = || "the boundary of the material is not a set of closed loops".to_string();
    let mut used = HashSet::new();
    let mut loops = Vec::new();
    for start in &boundary {
        let mut side = *start;
        let mut keys = Loop::new();
        while used.insert(side) {
            keys.push((side.0, at[&side.0]));
            side = next(side).ok_or_else(open)?;
        }
        if side != *start {
            return Err(open());
        }
        if !keys.is_empty() {
            loops.push(keys);
        }
    }
    Ok(loops)
}

/// The edges on the edge of the mesh that `key` is on.
fn edges_of(plane: &Plane, key: Key) -> Vec<(u32, u32)> {
    match key {
        Key::At(Corner::Node(n)) => plane.at_node.get(&n).cloned().unwrap_or_default(),
        Key::Cut(Corner::Node(a), Corner::Node(b)) => {
            let e = (a.min(b), a.max(b));
            plane.edges.get(&e).map(|e| vec![*e]).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Smooth the points `p` between the first and the last (all points when `closed`).
fn smooth(p: &mut [[f64; 2]], closed: bool) {
    let n = p.len();
    let inner = if closed { 0..n } else { 1..n.saturating_sub(1) };
    for _ in 0..SMOOTHING {
        for f in FACTORS {
            let old = p.to_vec();
            for i in inner.clone() {
                let (a, b) = (old[(i + n - 1) % n], old[(i + 1) % n]);
                for k in 0..2 {
                    p[i][k] += f * ((a[k] + b[k]) / 2.0 - old[i][k]);
                }
            }
        }
    }
}

/// `count` + 1 points at equal steps along the line through `p`, from its first point to its
/// last point.
fn resample(p: &[[f64; 2]], count: usize) -> Vec<[f64; 2]> {
    let length = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
    let mut along = vec![0.0];
    for w in p.windows(2) {
        along.push(along.last().unwrap() + length(w[0], w[1]));
    }
    let total = *along.last().unwrap();
    let mut out = vec![p[0]];
    let mut i = 0;
    for k in 1..count {
        let s = total * k as f64 / count as f64;
        while along[i + 1] < s {
            i += 1;
        }
        let f = (s - along[i]) / (along[i + 1] - along[i]).max(f64::MIN_POSITIVE);
        out.push([
            p[i][0] + f * (p[i + 1][0] - p[i][0]),
            p[i][1] + f * (p[i + 1][1] - p[i][1]),
        ]);
    }
    out.push(p[p.len() - 1]);
    out
}

fn length(p: &[[f64; 2]]) -> f64 {
    (p.windows(2))
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// True when `b` is on the line from `a` to `c`.
fn on_line(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
    let scale = (b[0] - a[0]).hypot(b[1] - a[1]) * (c[0] - b[0]).hypot(c[1] - b[1]);
    cross.abs() <= 1e-9 * scale
}

/// The gmsh script of the material of the `loops`, with the solid of thickness `thickness`.
/// Each node of `pins` on the edge of the mesh stays a point of the script.
fn geo(plane: &Plane, loops: &[Loop], pins: &HashSet<u32>, thickness: f64, metres: bool) -> String {
    let pinned = |key: Key| matches!(key, Key::At(Corner::Node(n)) if pins.contains(&n));
    let mut points: Vec<[f64; 2]> = Vec::new();
    let mut ids: HashMap<Key, usize> = HashMap::new();
    let mut add = |key: Option<Key>, xy: [f64; 2]| -> usize {
        if let Some(id) = key.and_then(|k| ids.get(&k)) {
            return *id;
        }
        points.push(xy);
        if let Some(k) = key {
            ids.insert(k, points.len());
        }
        points.len()
    };
    // Each curve: `Line` or `Spline`, and its points.
    let mut curves: Vec<(&str, Vec<usize>)> = Vec::new();
    let mut loop_curves: Vec<Vec<usize>> = Vec::new();
    for lp in loops {
        let n = lp.len();
        let on: Vec<Vec<(u32, u32)>> = (lp.iter()).map(|(k, _)| edges_of(plane, *k)).collect();
        let fixed: Vec<usize> = (0..n).filter(|i| !on[*i].is_empty()).collect();
        if fixed.is_empty() {
            // A loop in the mesh: 1 closed spline.
            let mut p: Vec<[f64; 2]> = lp.iter().map(|(_, xy)| *xy).collect();
            smooth(&mut p, true);
            p.push(p[0]);
            let count = ((length(&p) / plane.size).round() as usize).max(3);
            let mut q = resample(&p, count);
            q.pop();
            let mut tags: Vec<usize> = q.iter().map(|xy| add(None, *xy)).collect();
            tags.push(tags[0]);
            curves.push(("Spline", tags));
            loop_curves.push(vec![curves.len()]);
            continue;
        }
        // The pieces from each point on the edge of the mesh to the next one: a line on the edge
        // of the mesh, or a curve in the mesh.
        let m = fixed.len();
        let pieces: Vec<(usize, usize, bool)> = (0..m)
            .map(|k| {
                let (a, b) = (fixed[k], fixed[(k + 1) % m]);
                (
                    a,
                    b,
                    (a + 1) % n == b && on[a].iter().any(|e| on[b].contains(e)),
                )
            })
            .collect();
        // A line goes on through the start of piece `k` when both pieces are lines on 1 line.
        let through = |k: usize| {
            let (before, after) = (pieces[(k + m - 1) % m], pieces[k]);
            before.2
                && after.2
                && !pinned(lp[after.0].0)
                && on_line(lp[before.0].1, lp[after.0].1, lp[after.1].1)
        };
        let first = (0..m).find(|k| !through(*k)).unwrap_or(0);
        let mut own = Vec::new();
        let mut k = first;
        loop {
            let (a, mut b, straight) = pieces[k];
            k = (k + 1) % m;
            if straight {
                while k != first && through(k) {
                    b = pieces[k].1;
                    k = (k + 1) % m;
                }
                let tags = vec![add(Some(lp[a].0), lp[a].1), add(Some(lp[b].0), lp[b].1)];
                curves.push(("Line", tags));
            } else {
                let span: Vec<usize> = if b > a {
                    (a..=b).collect()
                } else {
                    (a..n).chain(0..=b).collect()
                };
                let mut p: Vec<[f64; 2]> = span.iter().map(|i| lp[*i].1).collect();
                smooth(&mut p, false);
                let count = ((length(&p) / plane.size).round() as usize).max(1);
                let q = resample(&p, count);
                let mut tags = vec![add(Some(lp[a].0), lp[a].1)];
                tags.extend(q[1..q.len() - 1].iter().map(|xy| add(None, *xy)));
                tags.push(add(Some(lp[b].0), lp[b].1));
                curves.push((if tags.len() > 2 { "Spline" } else { "Line" }, tags));
            }
            own.push(curves.len());
            if k == first {
                break;
            }
        }
        loop_curves.push(own);
    }
    let mut text = String::from(
        "// The CAD of the last design of the topology run: the material (the density 0.5 or \
         more), with\n// smoothed curves, and its solid. fh writes this file.\n\
         SetFactory(\"OpenCASCADE\");\n",
    );
    if metres {
        text.push_str(
            "// The model is in m. The STEP file is in mm.\nGeometry.OCCTargetUnit = \"M\";\n",
        );
    }
    for (i, p) in points.iter().enumerate() {
        text.push_str(&format!("Point({}) = {{{}, {}, 0}};\n", i + 1, p[0], p[1]));
    }
    for (i, (kind, tags)) in curves.iter().enumerate() {
        let list: Vec<String> = tags.iter().map(usize::to_string).collect();
        text.push_str(&format!("{kind}({}) = {{{}}};\n", i + 1, list.join(", ")));
    }
    let mut outer = Vec::new();
    let mut holes = Vec::new();
    for (i, (lp, own)) in loops.iter().zip(&loop_curves).enumerate() {
        let list: Vec<String> = own.iter().map(usize::to_string).collect();
        text.push_str(&format!(
            "Curve Loop({}) = {{{}}};\n",
            i + 1,
            list.join(", ")
        ));
        let xy: Vec<[f64; 2]> = lp.iter().map(|(_, p)| *p).collect();
        let a = area(&xy);
        if a > 0.0 {
            outer.push((i + 1, a, xy));
        } else {
            holes.push((i + 1, xy[0]));
        }
    }
    // A hole is in the smallest outer loop that holds its first point.
    let inside = |p: [f64; 2], poly: &[[f64; 2]]| {
        let n = poly.len();
        (0..n).fold(false, |c, i| {
            let (a, b) = (poly[i], poly[(i + n - 1) % n]);
            let crosses = (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            c ^ crosses
        })
    };
    let mut surfaces = Vec::new();
    for (tag, _, _) in &outer {
        surfaces.push(vec![*tag]);
    }
    for (tag, p) in &holes {
        let owner = (outer.iter().enumerate())
            .filter(|(_, (_, _, poly))| inside(*p, poly))
            .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1));
        if let Some((i, _)) = owner {
            surfaces[i].push(*tag);
        }
    }
    for (i, s) in surfaces.iter().enumerate() {
        let list: Vec<String> = s.iter().map(usize::to_string).collect();
        text.push_str(&format!(
            "Plane Surface({}) = {{{}}};\n",
            i + 1,
            list.join(", ")
        ));
    }
    let all = format!("1:{}", surfaces.len());
    text.push_str(&format!(
        "// The plane surfaces are the mesh of the check with ccx.\n\
         Physical Surface(\"DESIGN\") = {{{all}}};\n\
         Extrude {{0, 0, {thickness}}} {{ Surface{{{all}}}; }}\n"
    ));
    text
}

/// Run gmsh in `dir` with `args`, with the limits of the run. The output goes to `log.gmsh`.
fn gmsh(
    args: &[&str],
    dir: &Path,
    limits: (Instant, Option<std::time::Duration>, u64),
    cancel: &CancelToken,
) -> Result<(), String> {
    let program = Program {
        name: "gmsh",
        install: "Install gmsh (README.md) and put gmsh on PATH, or set FH_GMSH",
    };
    let log_file = dir.join("log.gmsh");
    let folder = |e: std::io::Error| format!("CAD folder: {e}");
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .map_err(folder)?;
    let mut cmd = Command::new(crate::msh::find_gmsh()?);
    cmd.args(args)
        .current_dir(dir)
        // gmsh reads `~/.gmsh-options`. The CAD folder has none.
        .env("HOME", dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(folder)?)
        .stderr(log);
    let status = supervise(&program, &mut cmd, limits, cancel, &mut || {})?;
    let log = fs::read_to_string(&log_file).unwrap_or_default();
    if !status.success() || log.lines().any(|l| l.trim().starts_with("Error")) {
        return Err(failure("gmsh", status, &log, &log_file));
    }
    Ok(())
}

fn nodes(model: &Model, target: &Target) -> Result<Vec<u32>, String> {
    match target {
        Target::Node(n) => Ok(vec![*n]),
        Target::Set(name) => (model.nset(name).map(|s| s.ids.clone()))
            .ok_or_else(|| format!("job.inp has no node set {name}")),
    }
}

/// A support: the first dof, the last dof and the value.
type Support = (u8, u8, f64);

/// The supports and the loads of the case.
struct Fixings {
    /// The supports of each node.
    supports: HashMap<u32, Vec<Support>>,
    /// Each edge on the edge of the mesh with the supports that both its ends have.
    lines: BTreeMap<(u32, u32), Vec<Support>>,
    /// The point loads: the node, the dof and the value.
    loads: Vec<(u32, u8, f64)>,
}

fn fixings(model: &Model, plane: &Plane) -> Result<Fixings, String> {
    let mut supports: HashMap<u32, Vec<Support>> = HashMap::new();
    let steps = model.steps.iter().flat_map(|s| &s.boundaries);
    for b in model.boundaries.iter().chain(steps) {
        for n in nodes(model, &b.target)? {
            let list = supports.entry(n).or_default();
            if !list.contains(&(b.first_dof, b.last_dof, b.value)) {
                list.push((b.first_dof, b.last_dof, b.value));
            }
        }
    }
    let mut lines = BTreeMap::new();
    for (a, b) in &plane.edges {
        if let (Some(sa), Some(sb)) = (supports.get(a), supports.get(b)) {
            let both: Vec<Support> = sa.iter().filter(|s| sb.contains(s)).copied().collect();
            if !both.is_empty() {
                lines.insert((*a, *b), both);
            }
        }
    }
    let mut loads = Vec::new();
    for load in model.steps.iter().flat_map(|s| &s.cloads) {
        for n in nodes(model, &load.target)? {
            loads.push((n, load.dof, load.magnitude));
        }
    }
    Ok(Fixings {
        supports,
        lines,
        loads,
    })
}

impl Fixings {
    /// The nodes that stay points of the CAD, so that the mesh of the check has a node there: each
    /// node with a load, and each node with a support that the lines of supports on its 2 sides do
    /// not give.
    fn pins(&self, plane: &Plane) -> HashSet<u32> {
        let given = |n: &u32, list: &Vec<Support>| {
            let sides = plane.at_node.get(n).map_or(&[][..], Vec::as_slice);
            !sides.is_empty()
                && sides.iter().all(|e| {
                    (self.lines.get(e)).is_some_and(|both| list.iter().all(|s| both.contains(s)))
                })
        };
        let supports = self.supports.iter().filter(|(n, list)| !given(n, list));
        (supports.map(|(n, _)| *n))
            .chain(self.loads.iter().map(|l| l.0))
            .collect()
    }
}

/// True when the sum `m` of the products of the rows of the supports has rank 3.
fn holds(m: [[f64; 3]; 3]) -> bool {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let scale = (m[0][0] + m[1][1] + m[2][2]) / 3.0;
    det > 1e-9 * scale.powi(3)
}

/// The parts of `mesh` (the elements that share nodes) that the supports `held` of the nodes at
/// `xy` let move as a rigid body in the plane, and the number of parts.
fn free_parts(
    mesh: &Model,
    xy: &HashMap<u32, [f64; 2]>,
    held: &HashMap<u32, Vec<Support>>,
) -> (usize, usize) {
    let mut root: HashMap<u32, u32> = HashMap::new();
    fn find(root: &mut HashMap<u32, u32>, n: u32) -> u32 {
        let up = *root.entry(n).or_insert(n);
        if up == n {
            return n;
        }
        let top = find(root, up);
        root.insert(n, top);
        top
    }
    for e in &mesh.elements {
        let a = find(&mut root, e.nodes[0]);
        for n in &e.nodes[1..] {
            let b = find(&mut root, *n);
            root.insert(b, a);
        }
    }
    // The rigid motion (a, b, w) moves the point (x, y) by (a - w y, b + w x). A support of dof
    // 1 or 2 stops 1 combination. The supports hold the part when they stop all 3.
    let (lo, hi) = (xy.values()).fold(([f64::MAX; 2], [f64::MIN; 2]), |(lo, hi), p| {
        (
            [lo[0].min(p[0]), lo[1].min(p[1])],
            [hi[0].max(p[0]), hi[1].max(p[1])],
        )
    });
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(f64::MIN_POSITIVE);
    let mut sums: BTreeMap<u32, [[f64; 3]; 3]> = BTreeMap::new();
    let nodes: Vec<u32> = root.keys().copied().collect();
    for n in nodes {
        let m = sums.entry(find(&mut root, n)).or_default();
        let (x, y) = ((xy[&n][0] - lo[0]) / span, (xy[&n][1] - lo[1]) / span);
        for (first, last, _) in held.get(&n).map_or(&[][..], Vec::as_slice) {
            for dof in (*first..=*last).filter(|d| (1..=2).contains(d)) {
                let r = if dof == 1 {
                    [1.0, 0.0, -y]
                } else {
                    [0.0, 1.0, x]
                };
                for i in 0..3 {
                    for j in 0..3 {
                        m[i][j] += r[i] * r[j];
                    }
                }
            }
        }
    }
    let free = sums.values().filter(|m| !holds(**m)).count();
    (free, sums.len())
}

/// The input of the check: the mesh `mesh` of gmsh, with the material of the density 1 and the
/// supports and the loads of the case (see the module doc).
fn check_input(
    plane: &Plane,
    mesh: &Model,
    fixings: &Fixings,
    s: &Settings,
    units: &str,
) -> Result<String, String> {
    let tol = 1e-6 * plane.size;
    let cell = |p: [f64; 2]| {
        (
            (p[0] / plane.size).floor() as i64,
            (p[1] / plane.size).floor() as i64,
        )
    };
    let mut grid: HashMap<(i64, i64), Vec<u32>> = HashMap::new();
    for (n, p) in &plane.xy {
        grid.entry(cell(*p)).or_default().push(*n);
    }
    let node_at = |p: [f64; 2]| -> Option<u32> {
        let (i, j) = cell(p);
        let near = (i - 1..=i + 1).flat_map(|i| (j - 1..=j + 1).map(move |j| (i, j)));
        let mut found: Vec<u32> = near
            .filter_map(|c| grid.get(&c))
            .flatten()
            .filter(|n| {
                let q = plane.xy[n];
                (q[0] - p[0]).hypot(q[1] - p[1]) <= tol
            })
            .copied()
            .collect();
        found.sort_unstable();
        found.first().copied()
    };
    let used: HashSet<u32> = mesh.elements.iter().flat_map(|e| e.nodes.clone()).collect();
    let mut cad_nodes: Vec<(u32, [f64; 2])> = (mesh.nodes.iter())
        .filter(|n| used.contains(&n.id))
        .map(|n| (n.id, [n.xyz[0], n.xyz[1]]))
        .collect();
    cad_nodes.sort_unstable_by_key(|n| n.0);
    let mut text = String::from(
        "** The check of the CAD of the topology run: the plane surfaces of design.geo, meshed by \
         gmsh,\n** with the material of the density 1 and the supports and the loads of the \
         case. fh writes this file.\n",
    );
    if matches!(units, "SI-mm" | "SI-m") {
        text.push_str(&format!("** units: {units}\n"));
    }
    text.push_str("*NODE\n");
    for (id, p) in &cad_nodes {
        text.push_str(&format!("{id}, {:.12e}, {:.12e}, 0\n", p[0], p[1]));
    }
    text.push_str(&format!("*ELEMENT, TYPE={}6, ELSET=CAD\n", plane.family));
    let xy: HashMap<u32, [f64; 2]> = cad_nodes.iter().copied().collect();
    for e in &mesh.elements {
        let mut n = e.nodes.clone();
        if area(&[xy[&n[0]], xy[&n[1]], xy[&n[2]]]) < 0.0 {
            n = vec![n[0], n[2], n[1], n[5], n[4], n[3]];
        }
        let list: Vec<String> = n.iter().map(u32::to_string).collect();
        text.push_str(&format!("{}, {}\n", e.id, list.join(", ")));
    }
    text.push_str(&format!(
        "*MATERIAL, NAME=CAD\n*ELASTIC\n{:.12e}, {:.12e}\n*SOLID SECTION, ELSET=CAD, MATERIAL=CAD\n",
        s.young, s.poisson
    ));
    if let Some(t) = s.thickness {
        text.push_str(&format!("{t:.12e}\n"));
    }
    text.push_str("*BOUNDARY\n");
    let mut held: HashMap<u32, Vec<Support>> = HashMap::new();
    for (id, p) in &cad_nodes {
        let mut list: Vec<Support> = node_at(*p)
            .and_then(|n| fixings.supports.get(&n).cloned())
            .unwrap_or_default();
        for ((a, b), both) in &fixings.lines {
            let (pa, pb) = (plane.xy[a], plane.xy[b]);
            let d = [pb[0] - pa[0], pb[1] - pa[1]];
            let f = ((p[0] - pa[0]) * d[0] + (p[1] - pa[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]);
            let off = (p[0] - pa[0] - f * d[0]).hypot(p[1] - pa[1] - f * d[1]);
            if (0.0..=1.0).contains(&f) && off <= tol {
                let more: Vec<Support> =
                    both.iter().filter(|s| !list.contains(s)).copied().collect();
                list.extend(more);
            }
        }
        for (first, last, value) in &list {
            text.push_str(&format!("{id}, {first}, {last}, {value:.12e}\n"));
        }
        held.insert(*id, list);
    }
    let (free, parts) = free_parts(mesh, &xy, &held);
    if free > 0 {
        return Err(format!(
            "the supports do not hold {free} of the {parts} parts of the CAD, thus ccx cannot \
             solve the check"
        ));
    }
    text.push_str("*STEP\n*STATIC\n*CLOAD\n");
    let cad_at: HashMap<u32, u32> = (cad_nodes.iter())
        .filter_map(|(id, p)| node_at(*p).map(|n| (n, *id)))
        .collect();
    for (n, dof, value) in &fixings.loads {
        let id = cad_at.get(n).ok_or_else(|| {
            format!("the load at node {n} is not on the edge of the material of the CAD")
        })?;
        text.push_str(&format!("{id}, {dof}, {value:.12e}\n"));
    }
    text.push_str("*EL PRINT, ELSET=CAD\nELSE, EVOL\n*NODE FILE, OUTPUT=3D\nU\n*END STEP\n");
    Ok(text)
}

/// Write the CAD of the densities `x` of the elements of `model` in `<work>/cad`, and check it
/// with `ccx` (see the module doc). `text` is the input of the case, and `volume` is the volume
/// of all elements. The limits of `ccx` count from `start`.
#[allow(clippy::too_many_arguments)]
pub(super) fn write(
    model: &Model,
    text: &str,
    x: &[f64],
    s: &Settings,
    volume: f64,
    ccx: &Ccx,
    start: Instant,
    cancel: &CancelToken,
    work: &Path,
) -> Result<Cad, String> {
    let plane = plane(model)?;
    let loops = outline(&plane, x)?;
    if loops.is_empty() {
        return Err("the design has no material with the density 0.5 or more".into());
    }
    let fixings = fixings(model, &plane)?;
    let units = fh_core::inp::units(text);
    let thickness = s.thickness.unwrap_or(1.0);
    let dir = work.join(DIR);
    let folder = |e: std::io::Error| format!("CAD folder: {e}");
    fs::create_dir_all(&dir).map_err(folder)?;
    let pins = fixings.pins(&plane);
    let script = geo(&plane, &loops, &pins, thickness, units == "SI-m");
    fs::write(dir.join("design.geo"), script).map_err(folder)?;
    let limits = (start, ccx.timeout, ccx.memory_limit);
    gmsh(
        &["design.geo", "-0", "-o", STEP, "-v", "2"],
        &dir,
        limits,
        cancel,
    )?;
    let check = || -> Result<Check, String> {
        let words: Vec<String> = (text.lines())
            .filter(|l| l.starts_with('*') && !l.starts_with("**"))
            .map(keyword)
            .collect();
        if let Some(w) = words.iter().find(|w| !TAKES.contains(&w.as_str())) {
            return Err(format!(
                "the check takes only supports (*BOUNDARY) and point loads (*CLOAD), and job.inp \
                 has {w}"
            ));
        }
        let size = format!("{}", plane.size / 2.0);
        let args = [
            "design.geo",
            "-2",
            "-order",
            "2",
            "-clmax",
            &size,
            "-format",
            "msh41",
            "-o",
            "mesh.msh",
            "-v",
            "2",
        ];
        gmsh(&args, &dir, limits, cancel)?;
        let msh = fs::read_to_string(dir.join("mesh.msh")).map_err(|e| format!("mesh.msh: {e}"))?;
        let mesh = (crate::msh::read_msh_shell(&msh))
            .map_err(|e| format!("the mesh of the CAD is not readable: {e}"))?
            .model;
        if mesh.elements.is_empty() {
            return Err("gmsh wrote no element of the CAD".into());
        }
        let job = check_input(&plane, &mesh, &fixings, s, &units)?;
        fs::write(dir.join("job.inp"), job).map_err(folder)?;
        let mut each = ccx.clone();
        each.timeout = ccx.timeout.map(|t| t.saturating_sub(start.elapsed()));
        let solve = dir.join("solve");
        each.run_with(&dir.join("job.inp"), &solve, cancel, &mut |_| {})?;
        let file = solve.join("ccx/job.dat");
        let dat = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let ids: Vec<u32> = mesh.elements.iter().map(|e| e.id).collect();
        let energy = element_values(&dat, "internal energy (element, energy)", &ids)?;
        let volumes = element_values(&dat, "volume (element, volume)", &ids)?;
        let check = Check {
            compliance: 2.0 * energy.iter().sum::<f64>(),
            volume: volumes.iter().sum::<f64>() / volume,
        };
        let table = format!(
            "# compliance volume\n{:.10e} {:.6}\n",
            check.compliance, check.volume
        );
        fs::write(dir.join(CHECK), table).map_err(folder)?;
        Ok(check)
    };
    Ok(Cad {
        step: dir.join(STEP),
        thickness,
        check: check(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fh_core::inp::parse;

    /// A plate of `nx` x `ny` square CPE4 elements of size 1. Element `i * ny + j + 1` is at
    /// column `i` and row `j`.
    fn grid(nx: u32, ny: u32) -> String {
        let node = |i: u32, j: u32| i * (ny + 1) + j + 1;
        let mut text = String::from("*NODE\n");
        for i in 0..=nx {
            for j in 0..=ny {
                text += &format!("{}, {i}, {j}\n", node(i, j));
            }
        }
        text += "*ELEMENT, TYPE=CPE4, ELSET=PLATE\n";
        for i in 0..nx {
            for j in 0..ny {
                let (a, b, c, d) = (
                    node(i, j),
                    node(i + 1, j),
                    node(i + 1, j + 1),
                    node(i, j + 1),
                );
                text += &format!("{}, {a}, {b}, {c}, {d}\n", i * ny + j + 1);
            }
        }
        text
    }

    fn areas(loops: &[Loop]) -> Vec<f64> {
        let xy = |lp: &Loop| lp.iter().map(|(_, p)| *p).collect::<Vec<_>>();
        loops.iter().map(|lp| area(&xy(lp))).collect()
    }

    #[test]
    fn a_hole_is_a_closed_spline_in_the_plane_surface_of_the_lines_on_the_edge() {
        let plane = plane(&parse(&grid(4, 4))).unwrap();
        let hole = |e: usize| (1..3).contains(&(e / 4)) && (1..3).contains(&(e % 4));
        let x: Vec<f64> = (0..16).map(|e| if hole(e) { 0.0 } else { 0.8 }).collect();
        let loops = outline(&plane, &x).unwrap();
        let a = areas(&loops);
        assert_eq!(a.len(), 2, "{a:?}");
        assert!(a.contains(&16.0), "{a:?}");
        assert!(a.iter().any(|a| (-9.0..-4.0).contains(a)), "{a:?}");

        let text = geo(&plane, &loops, &HashSet::new(), 2.0, false);
        assert_eq!(text.matches("\nLine(").count(), 4, "{text}");
        assert_eq!(text.matches("\nSpline(").count(), 1, "{text}");
        assert!(text.contains("\nPlane Surface(1) = {1, 2};\n"), "{text}");
        assert!(
            text.ends_with("Extrude {0, 0, 2} { Surface{1:1}; }\n"),
            "{text}"
        );
        assert!(!text.contains("OCCTargetUnit"), "{text}");
        assert!(geo(&plane, &loops, &HashSet::new(), 0.002, true).contains("OCCTargetUnit"));
    }

    #[test]
    fn parts_that_touch_at_a_node_are_separate_loops() {
        let plane = plane(&parse(&grid(2, 2))).unwrap();
        let loops = outline(&plane, &[1.0, 0.0, 0.0, 1.0]).unwrap();
        assert_eq!(areas(&loops), vec![1.0, 1.0]);
    }

    #[test]
    fn a_support_on_a_line_of_supports_is_not_a_point_of_the_cad() {
        let text = grid(3, 2)
            + "*NSET, NSET=SYMMETRY\n1, 2, 3\n*BOUNDARY\nSYMMETRY, 1, 1\n10, 2, 2\n\
               *STEP\n*STATIC\n*CLOAD\n3, 2, -1.0\n*END STEP\n";
        let model = parse(&text);
        let plane = plane(&model).unwrap();
        let fixings = fixings(&model, &plane).unwrap();
        let lines: Vec<_> = fixings.lines.iter().map(|(e, s)| (*e, s.clone())).collect();
        let x = vec![(1, 1, 0.0)];
        assert_eq!(lines, vec![((1, 2), x.clone()), ((2, 3), x)]);
        assert_eq!(fixings.loads, vec![(3, 2, -1.0)]);
        // Node 2 is between 2 lines of supports. Nodes 1 and 3 are at the ends.
        assert_eq!(fixings.pins(&plane), HashSet::from([1, 3, 10]));
    }

    #[test]
    fn a_part_that_can_move_on_its_supports_is_free() {
        let model = parse(&grid(2, 1));
        let xy = plane(&model).unwrap().xy;
        // Nodes 1 and 2 are at x = 0, and node 5 is at (2, 0).
        let mut held = HashMap::from([(1, vec![(1, 1, 0.0)]), (2, vec![(1, 2, 0.0)])]);
        assert_eq!(free_parts(&model, &xy, &held), (0, 1));
        held.insert(2, vec![(1, 1, 0.0)]);
        assert_eq!(free_parts(&model, &xy, &held), (1, 1));
        held.insert(5, vec![(2, 2, 0.0)]);
        assert_eq!(free_parts(&model, &xy, &held), (0, 1));
    }

    #[test]
    fn the_cad_needs_plane_elements() {
        let text = grid(1, 1).replace("CPE4", "C3D4");
        let error = plane(&parse(&text)).err().unwrap();
        assert_eq!(
            error,
            "the CAD needs a mesh of plane elements (CPS or CPE), and element 1 is C3D4"
        );
    }
}
