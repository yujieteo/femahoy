//! CalculiX (`ccx`) as a protocol v2 solver: `fh-solve-ccx <input.inp> <run-dir> [--formats vtu]`.
//! Decision D-13, see DESIGN.md.
//!
//! The adapter writes the model as `ccx` input in `<run-dir>/ccx`, runs the user's `ccx`
//! (`$FH_CCX`, else `ccx` on PATH), reads U and RF from the `.frd` file and the integration
//! point stress from the `.dat` file, and writes one VTU file for each step, `results.pvd` and `summary.toon`. `ccx` is GPL, so femahoy only starts it as a
//! separate program. The results are unvalidated.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use fh_inp::{Boundary, ElementTarget, Model, Target};
use fh_io::result::{base_grid, element_stress};
use fh_io::vtu::{write_pvd_units, write_vtu, Field, PvdEntry};
use fh_solver_api::{watch_stdin, PROTOCOL_HEADER, PVD_FILE, SUMMARY_FILE};
use fh_toon as toon;

/// Exit code of a run that the user cancelled.
const CANCELLED: u8 = 130;
/// Name of the `ccx` job in `<run-dir>/ccx`.
const JOB: &str = "job";

/// A real for `ccx` input. `ccx` reads at most 20 characters for each number.
fn real(x: f64) -> String {
    format!("{x:.14e}")
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

/// The `ccx` input for `model`. The parser has already flattened `*PART` and `*INSTANCE`.
/// Anything outside the linear subset is an error that names the keyword.
fn ccx_input(model: &Model) -> Result<String, String> {
    let unsupported = [
        (!model.equations.is_empty(), "*EQUATION"),
        (!model.mpcs.is_empty(), "*MPC"),
        (!model.couplings.is_empty(), "*COUPLING"),
        (!model.surfaces.is_empty(), "*SURFACE, TYPE=NODE"),
        (model.steps.iter().any(|s| s.nlgeom), "*STEP, NLGEOM"),
    ];
    if let Some((_, k)) = unsupported.iter().find(|(used, _)| *used) {
        return Err(format!(
            "{k} is not in the keyword subset of the ccx adapter"
        ));
    }
    let mut out = String::from("*NODE\n");
    for n in &model.nodes {
        let [x, y, z] = n.xyz.map(real);
        let _ = writeln!(out, "{}, {x}, {y}, {z}", n.id);
    }
    let mut kind = String::new();
    for e in &model.elements {
        let t = e.element_type.to_ascii_uppercase();
        if !matches!(t.as_str(), "C3D8" | "C3D8I" | "C3D10") {
            return Err(format!(
                "element {}: type {t} is not in the keyword subset of the ccx adapter",
                e.id
            ));
        }
        if t != kind {
            let _ = writeln!(out, "*ELEMENT, TYPE={t}");
        }
        kind = t;
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
        out.push_str("*NODE FILE\nU, RF\n*EL PRINT, ELSET=FH_ALL\nS\n*END STEP\n");
    }
    Ok(out)
}

/// The last increment of one step in the `.frd` file: node values by node id.
#[derive(Debug, Default)]
struct FrdStep {
    time: f64,
    increments: u64,
    disp: HashMap<u32, Vec<f64>>,
    forc: HashMap<u32, Vec<f64>>,
}

/// Read the ASCII `.frd` result blocks DISP and FORC. Step `i` of the model is
/// entry `i`. A later increment of a step replaces the values of the one before.
fn read_frd(text: &str) -> Result<Vec<FrdStep>, String> {
    let bad = |n: usize, why: &str| format!("{JOB}.frd line {}: {why}", n + 1);
    let mut steps: Vec<FrdStep> = Vec::new();
    let (mut step, mut inc, mut time) = (0usize, 0u64, 0.0);
    let mut block: Option<&str> = None;
    let mut ended = false;
    for (n, line) in text.lines().enumerate() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.first().copied() {
            Some("1PSTEP") => {
                let num = |i: usize| words.get(i).and_then(|w| w.parse().ok());
                (inc, step) = match (num(2), num(3)) {
                    (Some(i), Some(s)) if s > 0 => (i, s as usize),
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
                    (s.time, s.increments) = (time, inc);
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

/// The integration points of `ccx` in the order of `job.dat`, with their weights. `C3D8` and
/// `C3D8I`: 2 x 2 x 2 Gauss points, xi changes first, then eta, then zeta. `C3D10`: 4 points,
/// point k is near corner k.
fn ccx_points(kind: &str) -> Vec<([f64; 3], f64)> {
    if kind == "C3D10" {
        let (a, b) = (0.138_196_601_125_011, 0.585_410_196_624_968);
        let p = [[a, a, a], [b, a, a], [a, b, a], [a, a, b]];
        p.map(|p| (p, 1.0 / 24.0)).to_vec()
    } else {
        let g = 1.0 / 3f64.sqrt();
        let at = |i: usize, k: usize| if i >> k & 1 == 1 { g } else { -g };
        (0..8)
            .map(|i| ([at(i, 0), at(i, 1), at(i, 2)], 1.0))
            .collect()
    }
}

/// The derivatives of the shape functions by the natural coordinates `p`, in node order.
fn shape_derivatives(kind: &str, p: [f64; 3]) -> Vec<[f64; 3]> {
    if kind == "C3D10" {
        // Corner 1 is at p = 0, corners 2, 3 and 4 are at p = 1 on the axes. The midside
        // nodes 5 to 10 are on the edges 1-2, 2-3, 3-1, 1-4, 2-4 and 3-4.
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
    } else {
        let corners = [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        let dn = |s: [f64; 3]| {
            let f: [f64; 3] = std::array::from_fn(|k| 1.0 + s[k] * p[k]);
            [s[0] * f[1] * f[2], f[0] * s[1] * f[2], f[0] * f[1] * s[2]].map(|v| 0.125 * v)
        };
        corners.map(dn).to_vec()
    }
}

/// The weight times the Jacobian determinant of each `ccx` integration point of an element
/// with the node coordinates `x`.
fn point_weights(kind: &str, x: &[[f64; 3]]) -> Vec<f64> {
    let weight = |(p, w): ([f64; 3], f64)| {
        let mut j = [[0.0; 3]; 3];
        for (x, d) in x.iter().zip(shape_derivatives(kind, p)) {
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
    ccx_points(kind).into_iter().map(weight).collect()
}

/// Write the VTU file of each step, the PVD collection and the summary. Each file is written
/// under a temporary name and renamed (D-09).
fn write_results(
    model: &Model,
    frd: &[FrdStep],
    dat: &[Points],
    run_dir: &Path,
    header: toon::Header,
    solver: String,
    wall: f64,
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
            point_weights(&e.element_type, &x)
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
    let mut steps = Vec::new();
    let mut increments = Vec::new();
    for (i, step) in model.steps.iter().enumerate() {
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
            let points = dat[i].get(&e.id).ok_or_else(|| at("no stress"))?;
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
        let name = step
            .name
            .clone()
            .unwrap_or_else(|| format!("Step-{}", i + 1));
        steps.push(toon::RunStep {
            name: name.clone(),
            status: "converged".into(),
            reached: 1.0,
            increments: r.increments,
        });
        increments.push(toon::Increment {
            step: name,
            inc: r.increments,
            iters: 0,
            result: toon::IncResult::Ok,
        });
    }
    atomic(
        PVD_FILE,
        write_pvd_units(&entries, &header.units).as_bytes(),
    )?;
    let run = toon::Run {
        header,
        run_id: run_dir
            .file_name()
            .map_or_else(|| "run".into(), |n| n.to_string_lossy().to_string()),
        solver,
        results: PVD_FILE.into(),
        wall_s: wall,
        steps,
        increments,
        contact: None,
        failure: None,
        diagnostics: Vec::new(),
        next: Vec::new(),
    };
    let text = run
        .render(&toon::Opts::default())
        .map_err(|e| e.to_string())?;
    atomic(SUMMARY_FILE, text.as_bytes())
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

fn run(inp: &Path, run_dir: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let clock = Instant::now();
    let text = fs::read_to_string(inp).map_err(|e| format!("{}: {e}", inp.display()))?;
    let model = fh_inp::parse_file(&inp.display().to_string(), &text)
        .map_err(|e| fh_inp::join_errors(&e))?;
    let input = ccx_input(&model)?;
    let work = run_dir.join("ccx");
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    fs::write(work.join(format!("{JOB}.inp")), input).map_err(|e| format!("{JOB}.inp: {e}"))?;
    let exe = std::env::var_os("FH_CCX").map_or_else(|| PathBuf::from("ccx"), PathBuf::from);
    let log = fs::File::create(work.join(format!("{JOB}.log"))).map_err(|e| e.to_string())?;
    let mut child = Command::new(&exe)
        .args(["-i", JOB])
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(log.try_clone().map_err(|e| e.to_string())?)
        .stderr(log)
        .spawn()
        .map_err(|e| {
            format!(
                "cannot start {}: {e}. Install CalculiX and put ccx on PATH, or set FH_CCX",
                exe.display()
            )
        })?;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let log = fs::read_to_string(work.join(format!("{JOB}.log"))).unwrap_or_default();
    let error = log.lines().find(|l| l.contains("*ERROR"));
    if !status.success() || error.is_some() {
        return Err(format!(
            "ccx failed ({status}): {}",
            error.map_or("see ccx/job.log", str::trim)
        ));
    }
    let read = |ext: &str| {
        fs::read_to_string(work.join(format!("{JOB}.{ext}")))
            .map_err(|e| format!("{JOB}.{ext}: {e}"))
    };
    let (frd, dat) = (read_frd(&read("frd")?)?, read_dat(&read("dat")?));
    let version = log
        .split("CalculiX Version ")
        .nth(1)
        .and_then(|v| v.split([',', ' ', '\n']).next())
        .unwrap_or("unknown");
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325, |h: u64, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let header = toon::Header {
        source: inp
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string()),
        source_hash: format!("{hash:016x}"),
        units: input_units(&text),
        status: toon::Status::Ok,
    };
    let solver = format!("ccx {version}, unvalidated");
    let wall = clock.elapsed().as_secs_f64();
    write_results(&model, &frd, &dat, run_dir, header, solver, wall)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `fh validate` asks for VTU only, which is all that the adapter writes.
    let (inp, run_dir) = match args.as_slice() {
        [i, r] => (i, r),
        [i, r, f, v] if f == "--formats" && v == "vtu" => (i, r),
        _ => {
            eprintln!("usage: fh-solve-ccx <input.inp> <run-dir> [--formats vtu]");
            return ExitCode::from(2);
        }
    };
    println!("{PROTOCOL_HEADER}");
    let cancel = watch_stdin();
    match run(Path::new(inp), Path::new(run_dir), &cancel) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e == "cancelled" => {
            eprintln!("cancelled");
            ExitCode::from(CANCELLED)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
