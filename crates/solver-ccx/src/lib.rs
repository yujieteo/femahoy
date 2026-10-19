//! CalculiX (`ccx`) as the solver of femahoy, in the process of `fh` (D-13, see DESIGN.md).
//!
//! [`Ccx::run`] writes the model as `ccx` input in `<run-dir>/ccx`, runs the user's `ccx`
//! (`$FH_CCX`, else `ccx` on PATH), reads U and RF from the `.frd` file and the integration
//! point stress from the `.dat` file, and writes one VTU file for each step, `results.pvd`
//! in the run folder. `ccx` is GPL, so femahoy only starts it as a separate
//! program.
//!
//! # Limits
//!
//! `ccx` leads its own process group. The run kills the whole group at a cancel, at the
//! time limit or at the memory limit. The memory limit counts the physical footprint on
//! macOS and the resident memory elsewhere. Linux also gives `ccx` a hard limit that the
//! kernel enforces. If the memory cannot be read, the run stops `ccx` and tells this. The
//! default memory limit is 85 % of the machine RAM. There is no default time limit.
//!
//! A guard process in the group of `ccx` reads a pipe from this process. When this process
//! ends, also on SIGKILL, the pipe closes and the guard kills the group. Thus `ccx` never
//! lives longer than `fh`.

mod sys;

use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fh_inp::{Boundary, ElementTarget, Model, Target, Temperature};
use fh_io::result::{base_grid, element_stress};
use fh_io::vtu::{write_pvd_units, write_vtu, Field, PvdEntry};

pub use sys::total_memory;

pub const PVD_FILE: &str = "results.pvd";
/// A run puts this file in a run folder that it made. It clears only a folder that has the
/// file.
pub const RUN_MARKER: &str = ".fh-run";
/// Name of the `ccx` job in `<run-dir>/ccx`.
const JOB: &str = "job";

/// Stable diagnostic codes of a run. `docs/diagnostics.md` lists them.
pub mod code {
    pub const CANCELLED: &str = "API001";
    pub const RUN_FOLDER: &str = "API002";
    pub const START_FAILED: &str = "API003";
    pub const MEMORY: &str = "API005";
    pub const TIME_LIMIT: &str = "API006";
    pub const SOLVER_FAILED: &str = "API007";
}

/// An error of a run, with a stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Error {
            code,
            message: message.into(),
        }
    }

    /// The error of a run that the user cancelled.
    pub fn cancelled() -> Self {
        Error::new(code::CANCELLED, "run cancelled")
    }

    pub fn is_cancelled(&self) -> bool {
        self.code == code::CANCELLED
    }
}

/// An error of the model or of the `ccx` files.
fn failed(message: impl Into<String>) -> Error {
    Error::new(code::SOLVER_FAILED, message)
}

/// A flag that asks a running solve to stop. Clones share the flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 85 % of the machine RAM. If the system does not tell the RAM, there is no limit.
pub fn default_memory_limit() -> u64 {
    total_memory().map_or(u64::MAX, |ram| ram / 100 * 85)
}

/// A real for `ccx` input. `ccx` reads at most 20 characters for each number, and a longer
/// number loses the end of its exponent. Thus a negative number with a 2-digit exponent
/// gets one digit less.
fn real(x: f64) -> String {
    (0..=14)
        .rev()
        .map(|digits| format!("{x:.digits$e}"))
        .find(|s| s.len() <= 20)
        .unwrap_or_else(|| format!("{x:e}"))
}

fn target(t: &Target) -> String {
    match t {
        Target::Node(n) => n.to_string(),
        Target::Set(s) => s.clone(),
    }
}

/// Write `ids` 16 to a line.
fn id_lines(out: &mut String, ids: &[u32]) {
    for chunk in ids.chunks(16) {
        let line: Vec<String> = chunk.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}", line.join(", "));
    }
}

fn boundaries(out: &mut String, list: &[Boundary]) {
    if !list.is_empty() {
        out.push_str("*BOUNDARY\n");
    }
    for b in list {
        let _ = writeln!(
            out,
            "{}, {}, {}, {}",
            target(&b.target),
            b.first_dof,
            b.last_dof,
            real(b.value)
        );
    }
}

/// Node temperatures under `keyword`, with the same meaning in `ccx`.
fn temperatures(out: &mut String, keyword: &str, list: &[Temperature]) {
    if !list.is_empty() {
        let _ = writeln!(out, "{keyword}");
    }
    for t in list {
        let _ = writeln!(out, "{}, {}", target(&t.target), real(t.value));
    }
}

/// The `ccx` input for `model`. The parser refuses each keyword outside the linear subset.
fn ccx_input(model: &Model) -> Result<String, String> {
    let mut out = String::from("*NODE\n");
    for n in &model.nodes {
        let [x, y, z] = n.xyz.map(real);
        let _ = writeln!(out, "{}, {x}, {y}, {z}", n.id);
    }
    // The parser accepts only C3D10.
    out.push_str("*ELEMENT, TYPE=C3D10\n");
    for e in &model.elements {
        let nodes: Vec<String> = e.nodes.iter().map(u32::to_string).collect();
        let _ = writeln!(out, "{}, {}", e.id, nodes.join(", "));
    }
    for s in &model.nsets {
        let _ = writeln!(out, "*NSET, NSET={}", s.name);
        id_lines(&mut out, &s.ids);
    }
    for s in &model.elsets {
        let _ = writeln!(out, "*ELSET, ELSET={}", s.name);
        id_lines(&mut out, &s.ids);
    }
    for s in &model.element_surfaces {
        let _ = writeln!(out, "*SURFACE, NAME={}, TYPE=ELEMENT", s.name);
        for (e, f) in &s.faces {
            let _ = writeln!(out, "{e}, S{f}");
        }
    }
    for m in &model.materials {
        let e = m
            .elastic
            .as_ref()
            .ok_or_else(|| format!("material {} has no *ELASTIC", m.name))?;
        let _ = writeln!(
            out,
            "*MATERIAL, NAME={}\n*ELASTIC\n{}, {}",
            m.name,
            real(e.youngs_modulus),
            real(e.poissons_ratio)
        );
        if let Some(alpha) = m.expansion {
            let _ = writeln!(out, "*EXPANSION\n{}", real(alpha));
        }
    }
    out.push_str("*ELSET, ELSET=FH_ALL\n");
    id_lines(
        &mut out,
        &model.elements.iter().map(|e| e.id).collect::<Vec<_>>(),
    );
    // `ccx` needs a section. The femahoy subset gives the only material to all elements.
    if model.sections.is_empty() {
        let [m] = model.materials.as_slice() else {
            return Err("the model has no *SOLID SECTION and more than one material".into());
        };
        let _ = writeln!(out, "*SOLID SECTION, ELSET=FH_ALL, MATERIAL={}", m.name);
    }
    for s in &model.sections {
        let _ = writeln!(
            out,
            "*SOLID SECTION, ELSET={}, MATERIAL={}",
            s.elset, s.material
        );
    }
    boundaries(&mut out, &model.boundaries);
    temperatures(
        &mut out,
        "*INITIAL CONDITIONS, TYPE=TEMPERATURE",
        &model.initial_temperatures,
    );
    for step in &model.steps {
        out.push_str("*STEP\n*STATIC\n");
        boundaries(&mut out, &step.boundaries);
        if !step.cloads.is_empty() {
            out.push_str("*CLOAD\n");
        }
        for c in &step.cloads {
            let _ = writeln!(
                out,
                "{}, {}, {}",
                target(&c.target),
                c.dof,
                real(c.magnitude)
            );
        }
        if !step.dloads.is_empty() {
            out.push_str("*DLOAD\n");
        }
        for d in &step.dloads {
            let t = match &d.target {
                ElementTarget::Element(e) => e.to_string(),
                ElementTarget::Set(s) => s.clone(),
            };
            let _ = writeln!(out, "{t}, P{}, {}", d.face, real(d.pressure));
        }
        if !step.dsloads.is_empty() {
            out.push_str("*DSLOAD\n");
        }
        for d in &step.dsloads {
            let _ = writeln!(out, "{}, P, {}", d.surface, real(d.pressure));
        }
        temperatures(&mut out, "*TEMPERATURE", &step.temperatures);
        out.push_str("*NODE FILE\nU, RF\n*EL PRINT, ELSET=FH_ALL\nS\n*END STEP\n");
    }
    Ok(out)
}

/// The rigid body motions, in the order of the support check.
const MOTIONS: [&str; 6] = [
    "translation in x",
    "translation in y",
    "translation in z",
    "rotation about x",
    "rotation about y",
    "rotation about z",
];

/// `ccx` solves a model that can move as a rigid body with no error, and the result is wrong.
/// Thus in each step, the supports of each connected part of the mesh must hold the 6 rigid
/// body motions: on the held components, the 6 motions must be linearly independent.
fn check_support(model: &Model) -> Result<(), String> {
    fn root(up: &mut [usize], mut i: usize) -> usize {
        while up[i] != i {
            up[i] = up[up[i]];
            i = up[i];
        }
        i
    }
    let n = model.nodes.len();
    let index: HashMap<u32, usize> = (model.nodes.iter().enumerate())
        .map(|(i, node)| (node.id, i))
        .collect();
    let (mut up, mut used): (Vec<usize>, _) = ((0..n).collect(), vec![false; n]);
    for e in &model.elements {
        let nodes: Vec<usize> = e
            .nodes
            .iter()
            .filter_map(|id| index.get(id).copied())
            .collect();
        for pair in nodes.windows(2) {
            let (a, b) = (root(&mut up, pair[0]), root(&mut up, pair[1]));
            up[a] = b;
        }
        nodes.into_iter().for_each(|i| used[i] = true);
    }
    let part: Vec<usize> = (0..n).map(|i| root(&mut up, i)).collect();
    // The centre and the size of each part, so that the motions have the same scale.
    let (mut centre, mut size) = (vec![[0.0; 3]; n], vec![0.0_f64; n]);
    let mut count = vec![0.0; n];
    for i in (0..n).filter(|&i| used[i]) {
        let c = &mut centre[part[i]];
        (0..3).for_each(|k| c[k] += model.nodes[i].xyz[k]);
        count[part[i]] += 1.0;
    }
    for i in (0..n).filter(|&i| used[i]) {
        let c = centre[part[i]].map(|x| x / count[part[i]]);
        let d = (0..3)
            .map(|k| (model.nodes[i].xyz[k] - c[k]).powi(2))
            .sum::<f64>();
        size[part[i]] = size[part[i]].max(d.sqrt());
    }
    let hold = |held: &mut [u8], list: &[Boundary]| {
        for b in list {
            let ids = match &b.target {
                Target::Node(id) => vec![*id],
                Target::Set(s) => model.nset(s).map(|s| s.ids.clone()).unwrap_or_default(),
            };
            for i in ids.iter().filter_map(|id| index.get(id)) {
                (b.first_dof..=b.last_dof.min(3)).for_each(|d| held[*i] |= 1 << (d - 1));
            }
        }
    };
    let mut held = vec![0u8; n];
    hold(&mut held, &model.boundaries);
    for (s, step) in model.steps.iter().enumerate() {
        hold(&mut held, &step.boundaries);
        // For each part, the sum of r r^T over the held components r of the 6 motions.
        let mut gram: HashMap<usize, [[f64; 6]; 6]> = HashMap::new();
        for i in (0..n).filter(|&i| used[i]) {
            let p = part[i];
            let g = gram.entry(p).or_insert([[0.0; 6]; 6]);
            let c = centre[p].map(|x| x / count[p]);
            let d: [f64; 3] = std::array::from_fn(|k| (model.nodes[i].xyz[k] - c[k]) / size[p]);
            for k in (0..3).filter(|k| held[i] >> k & 1 == 1) {
                // The component k of each motion: a unit translation, or e x d.
                let r: [f64; 6] = std::array::from_fn(|m| match m {
                    0..3 => f64::from(u8::from(m == k)),
                    _ if k == (m + 1) % 3 => -d[(m + 2) % 3],
                    _ if k == (m + 2) % 3 => d[(m + 1) % 3],
                    _ => 0.0,
                });
                for (a, row) in g.iter_mut().enumerate() {
                    row.iter_mut().zip(r).for_each(|(v, rb)| *v += r[a] * rb);
                }
            }
        }
        for (p, g) in &gram {
            if let Some(m) = free_motion(g) {
                let name = step
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Step-{}", s + 1));
                return Err(format!(
                    "step {name}: the part with node {} can move as a rigid body ({}). ccx \
                     gives no error for such a model, but its result is wrong. Add a \
                     *BOUNDARY (in a case: a [[model.boundary]]) that holds this motion",
                    model.nodes[*p].id, MOTIONS[m]
                ));
            }
        }
    }
    Ok(())
}

/// The first motion that the held components do not hold apart from the motions before it:
/// a Cholesky factor of `g` with a pivot test relative to the diagonal.
fn free_motion(g: &[[f64; 6]; 6]) -> Option<usize> {
    let mut l = [[0.0; 6]; 6];
    for k in 0..6 {
        let pivot = g[k][k] - (0..k).map(|j| l[k][j] * l[k][j]).sum::<f64>();
        if pivot <= 1e-12 * g[k][k] {
            return Some(k);
        }
        l[k][k] = pivot.sqrt();
        for i in k + 1..6 {
            l[i][k] = (g[i][k] - (0..k).map(|j| l[i][j] * l[k][j]).sum::<f64>()) / l[k][k];
        }
    }
    None
}

/// The last increment of one step in the `.frd` file: node values by node id.
#[derive(Debug, Default)]
struct FrdStep {
    time: f64,
    disp: HashMap<u32, Vec<f64>>,
    forc: HashMap<u32, Vec<f64>>,
}

/// Read the ASCII `.frd` result blocks DISP and FORC. Step `i` of the model is
/// entry `i`. A later increment of a step replaces the values of the one before.
fn read_frd(text: &str) -> Result<Vec<FrdStep>, String> {
    let bad = |n: usize, why: &str| format!("{JOB}.frd line {}: {why}", n + 1);
    let mut steps: Vec<FrdStep> = Vec::new();
    let (mut step, mut time) = (0usize, 0.0);
    let mut block: Option<&str> = None;
    let mut ended = false;
    for (n, line) in text.lines().enumerate() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("1PSTEP") => {
                let num = |i: usize| words.get(i).and_then(|w| w.parse::<usize>().ok());
                step = match (num(2), num(3)) {
                    (Some(_), Some(s)) if s > 0 => s,
                    _ => return Err(bad(n, "bad PSTEP line")),
                };
            }
            Some("100CL") => {
                time = words
                    .get(2)
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| bad(n, "bad time"))?;
            }
            Some("-4") => {
                block = match words.get(1).copied() {
                    Some(b @ ("DISP" | "FORC")) if step > 0 => Some(b),
                    _ => None,
                };
                if let Some(b) = block {
                    if steps.len() < step {
                        steps.resize_with(step, FrdStep::default);
                    }
                    let s = &mut steps[step - 1];
                    s.time = time;
                    if b == "DISP" {
                        s.disp.clear();
                    } else {
                        s.forc.clear();
                    }
                }
            }
            Some("-3") => block = None,
            Some("9999") => ended = true,
            _ if line.starts_with(" -1") && block.is_some() => {
                // Fixed columns: " -1", the node in 10 characters, then values of 12.
                let id = line.get(3..13).and_then(|s| s.trim().parse().ok());
                let values: Option<Vec<f64>> = (13..line.len())
                    .step_by(12)
                    .map(|i| line.get(i..(i + 12).min(line.len()))?.trim().parse().ok())
                    .collect();
                let (Some(id), Some(values)) = (id, values) else {
                    return Err(bad(n, "bad value line"));
                };
                let s = &mut steps[step - 1];
                let map = if block == Some("DISP") {
                    &mut s.disp
                } else {
                    &mut s.forc
                };
                map.insert(id, values);
            }
            _ => {}
        }
    }
    if !ended {
        return Err(format!("{JOB}.frd has no end line 9999"));
    }
    Ok(steps)
}

/// The integration point stress of each element: the point number and the stress.
type Points = HashMap<u32, Vec<(usize, [f64; 6])>>;

/// Read the integration point stress of the `*EL PRINT` blocks of the `.dat` file. Block `i`
/// is step `i`, because each step is one increment. The stress is in the femahoy order
/// `[xx, yy, zz, xy, yz, xz]`. `ccx` prints `sxy, sxz, syz`.
fn read_dat(text: &str) -> Vec<Points> {
    let mut blocks: Vec<Points> = Vec::new();
    for line in text.lines() {
        if line.trim_start().starts_with("stresses (elem") {
            blocks.push(HashMap::new());
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        let (Some(block), [e, p, v @ ..]) = (blocks.last_mut(), w.as_slice()) else {
            continue;
        };
        let v: Option<Vec<f64>> = v.iter().map(|x| x.parse().ok()).collect();
        if let (Ok(e), Ok(p), Some(&[xx, yy, zz, xy, xz, yz])) =
            (e.parse(), p.parse(), v.as_deref())
        {
            block
                .entry(e)
                .or_default()
                .push((p, [xx, yy, zz, xy, yz, xz]));
        }
    }
    blocks
}

/// The 4 integration points of a `C3D10` in `ccx`, in the order of `job.dat`, with their
/// weights. Point k is near corner k.
fn ccx_points() -> [([f64; 3], f64); 4] {
    let (a, b) = (0.138_196_601_125_011, 0.585_410_196_624_968);
    [[a, a, a], [b, a, a], [a, b, a], [a, a, b]].map(|p| (p, 1.0 / 24.0))
}

/// The derivatives of the `C3D10` shape functions by the natural coordinates `p`, in node
/// order. Corner 1 is at p = 0, corners 2, 3 and 4 are at p = 1 on the axes. The midside
/// nodes 5 to 10 are on the edges 1-2, 2-3, 3-1, 1-4, 2-4 and 3-4.
fn shape_derivatives(p: [f64; 3]) -> Vec<[f64; 3]> {
    let l = [1.0 - p[0] - p[1] - p[2], p[0], p[1], p[2]];
    let dl = [[-1.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut dn: Vec<[f64; 3]> = (0..4)
        .map(|i| dl[i].map(|d| (4.0 * l[i] - 1.0) * d))
        .collect();
    for (i, j) in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)] {
        dn.push(std::array::from_fn(|k| {
            4.0 * (l[i] * dl[j][k] + l[j] * dl[i][k])
        }));
    }
    dn
}

/// The weight times the Jacobian determinant of each `ccx` integration point of an element
/// with the node coordinates `x`.
fn point_weights(x: &[[f64; 3]]) -> Vec<f64> {
    let weight = |(p, w): ([f64; 3], f64)| {
        let mut j = [[0.0; 3]; 3];
        for (x, d) in x.iter().zip(shape_derivatives(p)) {
            for (r, row) in j.iter_mut().enumerate() {
                for (c, v) in row.iter_mut().enumerate() {
                    *v += x[r] * d[c];
                }
            }
        }
        let det = j[0][0] * (j[1][1] * j[2][2] - j[1][2] * j[2][1])
            - j[0][1] * (j[1][0] * j[2][2] - j[1][2] * j[2][0])
            + j[0][2] * (j[1][0] * j[2][1] - j[1][1] * j[2][0]);
        w * det
    };
    ccx_points().into_iter().map(weight).collect()
}

/// The units that the input declares in a `** units:` comment before the first keyword (D-18).
fn input_units(text: &str) -> String {
    let comment = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map_while(|l| l.strip_prefix("**"));
    for c in comment {
        if let Some(u) = c.trim().strip_prefix("units:") {
            if matches!(u.trim(), "SI-mm" | "SI-m") {
                return u.trim().into();
            }
        }
    }
    "unspecified".into()
}

/// Why `ccx` failed and what to do, from its exit status and its output `log`.
fn ccx_failure(status: ExitStatus, log: &str, log_file: &Path) -> String {
    let see = format!("The full ccx output is in {}", log_file.display());
    // The first `*ERROR` message: its line and the lines after it, up to an empty line.
    let mut lines = log.lines().skip_while(|l| !l.contains("*ERROR"));
    if let Some(first) = lines.next() {
        let rest = lines.take_while(|l| !l.trim().is_empty() && !l.contains("*ERROR"));
        let words: Vec<&str> = std::iter::once(first)
            .chain(rest)
            .flat_map(str::split_whitespace)
            .collect();
        return format!(
            "ccx failed ({status}): {}. Correct the model as the message tells. {see}",
            words.join(" ")
        );
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return format!(
            "ccx stopped on signal {signal} and gave no error message. Usually the cause is \
             too little memory for the model. Use a coarser mesh, or a computer with more \
             memory. {see}"
        );
    }
    let last = log.lines().map(str::trim).rfind(|l| !l.is_empty());
    format!(
        "ccx failed ({status}) with no *ERROR message. The last ccx line is: {}. {see}",
        last.unwrap_or("(no output)")
    )
}

/// Write the VTU file of each step and the PVD collection in `units`. Each file is written
/// under a temporary name and renamed (D-09).
fn write_results(
    model: &Model,
    frd: &[FrdStep],
    dat: &[Points],
    run_dir: &Path,
    units: &str,
) -> Result<(), String> {
    let atomic = |file: &str, bytes: &[u8]| -> Result<(), String> {
        let tmp = run_dir.join(format!("{file}.tmp"));
        fs::write(&tmp, bytes)
            .and_then(|()| fs::rename(&tmp, run_dir.join(file)))
            .map_err(|e| format!("{file}: {e}"))
    };
    let base = base_grid(model)?;
    let coords: HashMap<u32, [f64; 3]> = model.nodes.iter().map(|n| (n.id, n.xyz)).collect();
    let weights: Vec<Vec<f64>> = (model.elements.iter())
        .map(|e| {
            let x: Vec<[f64; 3]> = e.nodes.iter().map(|n| coords[n]).collect();
            point_weights(&x)
        })
        .collect();
    if dat.len() != model.steps.len() {
        return Err(format!(
            "{JOB}.dat has {} stress blocks for {} steps",
            dat.len(),
            model.steps.len()
        ));
    }
    let mut entries = Vec::new();
    for (i, stress) in dat.iter().enumerate() {
        let r = frd
            .get(i)
            .filter(|r| !r.disp.is_empty())
            .ok_or_else(|| format!("{JOB}.frd has no displacement of step {}", i + 1))?;
        let node = |map: &HashMap<u32, Vec<f64>>, id: u32, k: usize| -> Vec<f64> {
            let v = map.get(&id).map_or(&[][..], Vec::as_slice);
            (0..k).map(|c| v.get(c).copied().unwrap_or(0.0)).collect()
        };
        let mut g = base.clone();
        for (name, map) in [("U", &r.disp), ("RF", &r.forc)] {
            let v = model.nodes.iter().flat_map(|n| node(map, n.id, 3));
            g.point_data.push(Field::f64(name, 3, v.collect()));
        }
        let (mut s, mut mises) = (Vec::new(), Vec::new());
        for (e, w) in model.elements.iter().zip(&weights) {
            let at =
                |what: &str| format!("{JOB}.dat has {what} of element {} in step {}", e.id, i + 1);
            let points = stress.get(&e.id).ok_or_else(|| at("no stress"))?;
            let weighted = points
                .iter()
                .map(|(p, s)| Some((*s, *w.get(p.checked_sub(1)?)?)))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| at("a bad integration point"))?;
            let m = element_stress(weighted);
            let [xx, yy, zz, xy, yz, zx] = m;
            let d = (xx - yy).powi(2) + (yy - zz).powi(2) + (zz - xx).powi(2);
            mises.push((0.5 * d + 3.0 * (xy * xy + yz * yz + zx * zx)).sqrt());
            s.extend(m);
        }
        g.cell_data.push(Field::f64("S", 6, s));
        g.cell_data.push(Field::f64("MISES", 1, mises));
        let file = format!("step-{}.vtu", i + 1);
        atomic(&file, &write_vtu(&g).map_err(|e| e.to_string())?)?;
        entries.push(PvdEntry { time: r.time, file });
    }
    atomic(PVD_FILE, write_pvd_units(&entries, units).as_bytes())
}

/// The place of a running solve, from the output of `ccx`. Steps and increments count from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub step: u64,
    pub increment: u64,
}

/// Reads the new lines of the log of `ccx` and tells the place of the solve.
struct LogTail {
    file: PathBuf,
    pos: u64,
    last: Option<Progress>,
}

impl LogTail {
    fn poll(&mut self, on_progress: &mut dyn FnMut(Progress)) {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut f) = fs::File::open(&self.file) else {
            return;
        };
        let mut bytes = Vec::new();
        if f.seek(SeekFrom::Start(self.pos)).is_err() || f.read_to_end(&mut bytes).is_err() {
            return;
        }
        let Some(end) = bytes.iter().rposition(|b| *b == b'\n') else {
            return;
        };
        self.pos += end as u64 + 1;
        for line in String::from_utf8_lossy(&bytes[..end]).lines() {
            let mut words = line.split_whitespace();
            let (key, n) = (
                words.next(),
                words.next().and_then(|n| n.parse::<u64>().ok()),
            );
            let now = match (key, n, self.last) {
                (Some("STEP"), Some(step), _) => Progress { step, increment: 0 },
                (Some("INCREMENT"), Some(increment), Some(p)) => Progress { increment, ..p },
                _ => continue,
            };
            if self.last != Some(now) {
                self.last = Some(now);
                if now.increment > 0 {
                    on_progress(now);
                }
            }
        }
    }
}

/// The `ccx` program with the limits of its runs.
#[derive(Debug, Clone)]
pub struct Ccx {
    /// `$FH_CCX`, else `ccx` on PATH.
    pub exe: PathBuf,
    /// Time limit of a run. `None` means no limit: a large model can solve for hours.
    pub timeout: Option<Duration>,
    /// Limit for the process group of `ccx` in bytes: physical footprint on macOS, resident
    /// elsewhere. `u64::MAX` means no limit.
    pub memory_limit: u64,
}

impl Default for Ccx {
    fn default() -> Self {
        Self::new()
    }
}

const POLL: Duration = Duration::from_millis(10);
const PROBE_EVERY: Duration = Duration::from_millis(100);

/// What to do when a run does not fit in its limits.
const SMALLER: &str = "Use a coarser mesh (a larger element size) to make the model smaller";

impl Ccx {
    /// `ccx` from `$FH_CCX`, else `ccx` on PATH. It has no time limit. The memory limit is
    /// 85 % of the machine RAM.
    pub fn new() -> Self {
        Ccx {
            exe: std::env::var_os("FH_CCX").map_or_else(|| PathBuf::from("ccx"), PathBuf::from),
            timeout: None,
            memory_limit: default_memory_limit(),
        }
    }

    /// Solve `inp` in the folder `run_dir`. The run clears the folder first, if a run made
    /// the folder earlier.
    pub fn run(&self, inp: &Path, run_dir: &Path) -> Result<(), Error> {
        self.run_with(inp, run_dir, &CancelToken::new(), &mut |_| {})
    }

    /// Solve, and stop when `cancel` is set. `on_progress` gets the step and the increment
    /// that `ccx` starts.
    pub fn run_with(
        &self,
        inp: &Path,
        run_dir: &Path,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<(), Error> {
        if cancel.is_cancelled() {
            return Err(Error::cancelled());
        }
        // Check the input before the folder changes: a new run clears the folder.
        let inp = fs::canonicalize(inp).map_err(|e| {
            Error::new(
                code::START_FAILED,
                format!("{}: input file: {e}", inp.display()),
            )
        })?;
        let text =
            fs::read_to_string(&inp).map_err(|e| failed(format!("{}: {e}", inp.display())))?;
        let model = fh_inp::parse_file(&inp.display().to_string(), &text)
            .map_err(|e| failed(fh_inp::join_errors(&e)))?;
        let input = ccx_input(&model).map_err(failed)?;
        check_support(&model).map_err(failed)?;
        let run_dir = prepare_run_dir(run_dir, &inp)?;
        let work = run_dir.join("ccx");
        let folder = |e: std::io::Error| Error::new(code::RUN_FOLDER, format!("run folder: {e}"));
        fs::create_dir_all(&work).map_err(folder)?;
        fs::write(work.join(format!("{JOB}.inp")), input).map_err(folder)?;
        let log_file = work.join(format!("{JOB}.log"));
        let log = fs::File::create(&log_file).map_err(folder)?;
        let status = self.solve(&work, &log_file, log, cancel, on_progress)?;
        let log = fs::read_to_string(&log_file).unwrap_or_default();
        if !status.success() || log.contains("*ERROR") {
            return Err(failed(ccx_failure(status, &log, &log_file)));
        }
        let read = |ext: &str| {
            fs::read_to_string(work.join(format!("{JOB}.{ext}")))
                .map_err(|e| failed(format!("{JOB}.{ext}: {e}")))
        };
        let frd = read_frd(&read("frd")?).map_err(failed)?;
        let dat = read_dat(&read("dat")?);
        write_results(&model, &frd, &dat, &run_dir, &input_units(&text)).map_err(failed)
    }

    /// Run `ccx -i job` in `work` with its output in `log`, in its own process group, to its
    /// end or to a stop.
    fn solve(
        &self,
        work: &Path,
        log_file: &Path,
        log: fs::File,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(Progress),
    ) -> Result<ExitStatus, Error> {
        let cannot_start = |e: std::io::Error| {
            Error::new(
                code::START_FAILED,
                format!(
                    "cannot start {}: {e}. Install CalculiX (docs/ccx.md) and put ccx on PATH, \
                     or set FH_CCX",
                    self.exe.display()
                ),
            )
        };
        let limited = self.memory_limit != u64::MAX;
        // The guard leads the group. It stops the group when the pipe from this process
        // closes. It is the leader, so the group exists until the end of `ccx`.
        let mut guard = Command::new("/bin/sh");
        guard
            .args(["-c", "while read -r _; do :; done; kill -KILL 0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        sys::prepare(&mut guard, None, 0);
        let guard = guard.spawn().map_err(cannot_start)?;
        let group = sys::Group::new(guard.id());
        let mut tree = Tree {
            group,
            guard,
            ccx: None,
        };
        let mut cmd = Command::new(&self.exe);
        cmd.args(["-i", JOB])
            .current_dir(work)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(cannot_start)?)
            .stderr(log);
        sys::prepare(
            &mut cmd,
            limited.then_some(self.memory_limit),
            tree.group.pid(),
        );
        // From here each return and each panic drops `tree`, and the drop kills the group.
        tree.ccx = Some(cmd.spawn().map_err(cannot_start)?);
        let start = Instant::now();
        let mut last_probe = Instant::now();
        let mut tail = LogTail {
            file: log_file.to_owned(),
            pos: 0,
            last: None,
        };
        let mut last_tail = Instant::now();
        loop {
            if last_tail.elapsed() >= PROBE_EVERY {
                last_tail = Instant::now();
                tail.poll(on_progress);
            }
            if cancel.is_cancelled() {
                return Err(Error::cancelled());
            }
            if let Some(t) = self.timeout.filter(|t| start.elapsed() > *t) {
                return Err(Error::new(
                    code::TIME_LIMIT,
                    format!(
                        "the solve stopped at the time limit of {} s. {SMALLER}",
                        t.as_secs_f64()
                    ),
                ));
            }
            if limited && last_probe.elapsed() >= PROBE_EVERY {
                last_probe = Instant::now();
                match sys::probe(tree.group.pid()) {
                    Ok(b) if b > self.memory_limit => {
                        return Err(Error::new(
                            code::MEMORY,
                            format!(
                                "the solve stopped after ccx used more than the memory limit \
                                 of {}. {SMALLER}",
                                size(self.memory_limit)
                            ),
                        ));
                    }
                    Ok(_) => {}
                    Err(e) => {
                        return Err(Error::new(
                            code::MEMORY,
                            format!(
                                "the memory probe failed ({e}), so the memory limit cannot \
                                 apply"
                            ),
                        ));
                    }
                }
            }
            // The end of `ccx` comes after the checks, so a late end does not hide a cancel
            // or the time limit.
            let ccx = tree.ccx.as_mut().expect("ccx started");
            match ccx.try_wait() {
                Ok(Some(status)) => {
                    tail.poll(on_progress);
                    return Ok(status);
                }
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => return Err(failed(format!("wait for ccx failed: {e}"))),
            }
        }
    }
}

/// The process group of a run: the guard and `ccx`. A drop kills the group, so that an early
/// return or a panic cannot leave `ccx` running without its limits.
struct Tree {
    group: sys::Group,
    guard: Child,
    ccx: Option<Child>,
}

impl Drop for Tree {
    fn drop(&mut self) {
        // Also the processes that `ccx` left behind.
        self.group.kill();
        for child in std::iter::once(&mut self.guard).chain(&mut self.ccx) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A memory size for a person: GiB from 1 GiB, else MiB.
fn size(bytes: u64) -> String {
    let mib = bytes as f64 / f64::from(1 << 20);
    if mib >= 1024.0 {
        format!("{:.1} GiB", mib / 1024.0)
    } else {
        format!("{} MiB", (mib * 1000.0).round() / 1000.0)
    }
}

/// Make the run folder ready. A folder with files in it must be a run folder (it has the
/// marker file), or the call fails. A fresh run clears it, so the input `inp` (a real path)
/// must be outside it.
fn prepare_run_dir(run_dir: &Path, inp: &Path) -> Result<PathBuf, Error> {
    let fail = |message: String| Err(Error::new(code::RUN_FOLDER, message));
    let err = |e: std::io::Error| Error::new(code::RUN_FOLDER, format!("run folder: {e}"));
    fs::create_dir_all(run_dir).map_err(err)?;
    let dir = fs::canonicalize(run_dir).map_err(err)?;
    if inp.starts_with(&dir) {
        return fail(format!(
            "input file {} is inside the run folder, which a new run clears",
            inp.display()
        ));
    }
    let marked = dir.join(RUN_MARKER).is_file();
    let empty = fs::read_dir(&dir).map_err(err)?.next().is_none();
    if !empty && !marked {
        return fail(format!(
            "run folder {} has files and is not a run folder of femahoy: choose an empty folder",
            dir.display()
        ));
    }
    for entry in fs::read_dir(&dir).map_err(err)?.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            fs::remove_dir_all(&path).map_err(err)?;
        } else {
            fs::remove_file(&path).map_err(err)?;
        }
    }
    fs::write(dir.join(RUN_MARKER), "femahoy run folder\n").map_err(err)?;
    Ok(dir)
}
