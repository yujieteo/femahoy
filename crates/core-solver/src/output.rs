//! Results files for the subprocess protocol: for each step a VTU file (a text table and a
//! legacy VTK file are options in `Options::formats`), a PVD collection, a restart file and
//! the `fh.run` TOON summary.
//!
//! The writer takes one increment at a time from the solver and keeps only the
//! base grid in memory. Every file is written under a temporary name and renamed.
//! Decision D-09, see DESIGN.md.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use fh_inp::Model;
use fh_io::vtk::write_vtk_to;
use fh_io::vtu::{
    read_vtu, write_pvd_units, write_vtu_to, Field, Grid, PvdEntry, VTK_HEXAHEDRON,
    VTK_QUADRATIC_TETRA,
};
use fh_solver_api::{ApiError, RunReport, Solver, PVD_FILE, SUMMARY_FILE};
use fh_toon as toon;

use crate::element::Kind;
use crate::restart::{read_latest, Restart};
use crate::system::{solve_from, CANCELLED};
use crate::text::write_text;
use crate::{
    Formats, Increment, Observer, Options, Resume, Solution, SolverError, StepInfo, StepResult,
    Summary,
};

/// Name of the restart file in the run folder.
pub const RESTART_FILE: &str = "restart.fh";

/// Diagnostic code of a dof that no element reaches. The solver fixes it at zero.
pub const ORPHAN_CODE: &str = "SOL101";

/// The input file of a run: a name for the summary, the hash of its text, and its declared
/// units ([`input_units`]).
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub name: &'a str,
    pub hash: u64,
    pub units: &'a str,
}

/// The value of `units` in the results of an input with no declared units.
pub const NO_UNITS: &str = "unspecified";

/// The units that an input file declares (D-18): a comment line `** units: SI-mm` or
/// `** units: SI-m` before the first keyword. `fh model` writes it. Abaqus input has no unit
/// keyword, so any other input gives [`NO_UNITS`]. The solver uses the units only as a label.
pub fn input_units(text: &str) -> &'static str {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix("**") else {
            break;
        };
        if let Some(value) = comment.trim().strip_prefix("units:") {
            return match value.trim() {
                "SI-mm" => "SI-mm",
                "SI-m" => "SI-m",
                _ => NO_UNITS,
            };
        }
    }
    NO_UNITS
}

/// The reference solver. Its results are unvalidated until the suite passes.
pub struct CoreSolver;

impl Solver for CoreSolver {
    fn name(&self) -> &str {
        "core"
    }

    fn validated(&self) -> bool {
        false
    }

    fn solve(&self, model: &Model, run_dir: &Path) -> Result<RunReport, ApiError> {
        run_model(
            model,
            &Input {
                name: "model",
                hash: 0,
                units: NO_UNITS,
            },
            run_dir,
            &Options::default(),
            false,
            &mut |_| {},
            &|| false,
        )
    }
}

fn flat(v: &[[f64; 3]]) -> Vec<f64> {
    v.iter().flatten().copied().collect()
}

fn max_norm(v: &[[f64; 3]]) -> f64 {
    v.iter()
        .map(|a| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt())
        .fold(0.0, f64::max)
}

/// FNV-1a hash of the input text. The restart file keeps it, so that a
/// restart with a changed input is refused.
pub fn input_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The grid without result fields: points, cells, ids and sets.
fn base_grid(model: &Model) -> Result<Grid, ApiError> {
    let index: std::collections::HashMap<u32, u32> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    let mut grid = Grid {
        points: model.nodes.iter().map(|n| n.xyz).collect(),
        ..Grid::default()
    };
    for el in &model.elements {
        let kind = Kind::parse(&el.element_type).ok_or_else(|| {
            ApiError::solver_output(format!("element {}: unsupported type", el.id))
        })?;
        grid.cell_types.push(match kind {
            Kind::C3D10 => VTK_QUADRATIC_TETRA,
            _ => VTK_HEXAHEDRON,
        });
        let cell = el
            .nodes
            .iter()
            .map(|n| {
                index
                    .get(n)
                    .copied()
                    .ok_or_else(|| ApiError::solver_output(format!("node {n} not found")))
            })
            .collect::<Result<_, _>>()?;
        grid.cells.push(cell);
    }
    let ids = |what: &str, v: &[u32]| -> Result<Vec<i32>, ApiError> {
        v.iter()
            .map(|&i| {
                i32::try_from(i).map_err(|_| {
                    ApiError::solver_output(format!(
                        "{what} {i} is above {}: the 32-bit {what} array of the VTU file cannot hold it",
                        i32::MAX
                    ))
                })
            })
            .collect()
    };
    grid.point_data.push(Field::i32(
        "NODE_ID",
        ids(
            "node id",
            &model.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
        )?,
    ));
    grid.cell_data.push(Field::i32(
        "ELEMENT_ID",
        ids(
            "element id",
            &model.elements.iter().map(|e| e.id).collect::<Vec<_>>(),
        )?,
    ));
    for s in &model.nsets {
        let member: std::collections::HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .nodes
            .iter()
            .map(|n| i32::from(member.contains(&n.id)))
            .collect();
        grid.point_data
            .push(Field::i32(&format!("NSET_{}", s.name), flags));
    }
    for s in &model.elsets {
        let member: std::collections::HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .elements
            .iter()
            .map(|e| i32::from(member.contains(&e.id)))
            .collect();
        grid.cell_data
            .push(Field::i32(&format!("ELSET_{}", s.name), flags));
    }
    Ok(grid)
}

/// One row of the increment table of the summary: an accepted increment or a cutback.
#[derive(Debug, Clone, PartialEq)]
struct IncLine {
    step: usize,
    number: usize,
    /// Linear solves of the attempt.
    iters: usize,
    ok: bool,
}

/// Extension of the file that the PVD collection and the restart check use: the first grid
/// format that is on.
fn primary(f: Formats) -> &'static str {
    if f.vtu {
        "vtu"
    } else if f.vtk {
        "vtk"
    } else {
        "txt"
    }
}

fn io(what: &str, e: std::io::Error) -> ApiError {
    ApiError::solver_output(format!("{what}: {e}"))
}

/// Write a file under a temporary name and rename it. A stop never leaves half a file.
fn atomic(
    dir: &Path,
    file: &str,
    body: impl FnOnce(&mut BufWriter<fs::File>) -> Result<(), ApiError>,
) -> Result<(), ApiError> {
    let tmp = dir.join(format!("{file}.tmp"));
    let mut w = BufWriter::new(fs::File::create(&tmp).map_err(|e| io(file, e))?);
    #[cfg(feature = "restart-test-hooks")]
    crate::restart::pause_write(&tmp);
    body(&mut w)?;
    w.flush().map_err(|e| io(file, e))?;
    drop(w);
    fs::rename(&tmp, dir.join(file)).map_err(|e| io(file, e))
}

/// Name of the result files of an increment, without the extension. The last increment of a
/// step has the name of the step.
fn result_name(step: usize, number: usize, last: bool) -> String {
    if last {
        format!("step-{}", step + 1)
    } else {
        format!("step-{}-inc-{number}", step + 1)
    }
}

/// The lines of the history text of a restart file.
fn history_text(entries: &[PvdEntry], incs: &[IncLine], part: Option<&StepInfo>) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&format!("entry {:e} {}\n", e.time, e.file));
    }
    for i in incs {
        let result = if i.ok { "ok" } else { "cutback" };
        s.push_str(&format!(
            "inc {} {} {} {result}\n",
            i.step, i.number, i.iters
        ));
    }
    if let Some(i) = part {
        s.push_str(&format!(
            "part {} {} {} {:e} {} {} {}\n",
            i.free_dofs,
            method_name(i),
            i.iterations,
            i.residual,
            i.increments,
            i.cutbacks,
            i.newton_iterations
        ));
    }
    s
}

/// What a restart file keeps for the writer: PVD entries, increment rows, and the totals of
/// the step that is not complete.
type History = (Vec<PvdEntry>, Vec<IncLine>, Option<StepInfo>);

/// The inverse of [`history_text`].
fn parse_history(text: &str) -> Result<History, ApiError> {
    let bad = || ApiError::solver_output(format!("{RESTART_FILE}: bad history line"));
    let (mut entries, mut incs, mut part) = (Vec::new(), Vec::new(), None);
    for l in text.lines() {
        if let Some(r) = l.strip_prefix("entry ") {
            let (t, f) = r.split_once(' ').ok_or_else(bad)?;
            entries.push(PvdEntry {
                time: t.parse().map_err(|_| bad())?,
                file: f.to_string(),
            });
        } else if let Some(r) = l.strip_prefix("inc ") {
            let w: Vec<&str> = r.split_whitespace().collect();
            if w.len() != 4 || !matches!(w[3], "ok" | "cutback") {
                return Err(bad());
            }
            let num = |i: usize| w[i].parse::<usize>().map_err(|_| bad());
            incs.push(IncLine {
                step: num(0)?,
                number: num(1)?,
                iters: num(2)?,
                ok: w[3] == "ok",
            });
        } else if let Some(r) = l.strip_prefix("part ") {
            let w: Vec<&str> = r.split_whitespace().collect();
            if w.len() != 7 {
                return Err(bad());
            }
            let num = |i: usize| w[i].parse::<usize>().map_err(|_| bad());
            part = Some(StepInfo {
                free_dofs: num(0)?,
                method: if w[1] == "pcg" {
                    crate::Method::Pcg
                } else {
                    crate::Method::Cholesky
                },
                iterations: num(2)?,
                residual: w[3].parse().map_err(|_| bad())?,
                increments: num(4)?,
                cutbacks: num(5)?,
                newton_iterations: num(6)?,
            });
        } else {
            return Err(bad());
        }
    }
    Ok((entries, incs, part))
}

/// Delete the temporary files and the result files of increments that the restart point does
/// not include. The run writes them again.
fn clean_run_folder(dir: &Path, r: &Restart) -> Result<(), ApiError> {
    let done = |step0: usize, number: Option<usize>| match number {
        Some(n) => step0 < r.step || (step0 == r.step && n <= r.number),
        None => step0 < r.step || (step0 == r.step && r.complete),
    };
    for e in fs::read_dir(dir).map_err(|e| io("run folder", e))? {
        let e = e.map_err(|e| io("run folder", e))?;
        let name = e.file_name().to_string_lossy().to_string();
        let stale = if name.ends_with(".tmp") {
            true
        } else if let Some(rest) = name.strip_prefix("step-") {
            let stem = rest.rsplit_once('.').map_or(rest, |x| x.0);
            let mut p = stem.split("-inc-");
            let step = p.next().and_then(|s| s.parse::<usize>().ok());
            let number = p.next().map(|s| s.parse::<usize>().ok());
            match (step, number) {
                (Some(s), None) if s >= 1 => !done(s - 1, None),
                (Some(s), Some(Some(n))) if s >= 1 => !done(s - 1, Some(n)),
                _ => false,
            }
        } else {
            false
        };
        if stale {
            fs::remove_file(e.path()).map_err(|e| io(&name, e))?;
        }
    }
    Ok(())
}

/// Check the result files of the increments that a restart keeps. Each file of each format
/// that is on must exist, and a VTU file must read. A damaged or lost file would leave a PVD
/// collection that points to bad data.
fn check_kept_results(dir: &Path, entries: &[PvdEntry], f: Formats) -> Result<(), ApiError> {
    for e in entries {
        let stem = e.file.rsplit_once('.').map_or(e.file.as_str(), |x| x.0);
        for (on, ext) in [(f.vtu, "vtu"), (f.vtk, "vtk"), (f.text, "txt")] {
            if !on {
                continue;
            }
            let file = format!("{stem}.{ext}");
            let path = dir.join(&file);
            let problem = if ext == "vtu" {
                match fs::read(&path) {
                    Ok(b) => read_vtu(&b).err().map(|e| e.to_string()),
                    Err(e) => Some(e.to_string()),
                }
            } else {
                match fs::metadata(&path) {
                    Ok(m) if m.is_file() && m.len() > 0 => None,
                    Ok(_) => Some("the file is empty".into()),
                    Err(e) => Some(e.to_string()),
                }
            };
            if let Some(why) = problem {
                return Err(ApiError::solver_output(format!(
                    "restart: the result file {file} of a kept increment is missing or damaged ({why}). Run again without restart"
                )));
            }
        }
    }
    Ok(())
}

struct Writer<'a> {
    run_dir: &'a Path,
    formats: Formats,
    hash: u64,
    units: &'a str,
    grid: Grid,
    n_point: usize,
    n_cell: usize,
    restart_every: usize,
    restart_minutes: Option<f64>,
    last_restart: Instant,
    entries: Vec<PvdEntry>,
    incs: Vec<IncLine>,
    emit: &'a mut dyn FnMut(&str),
    cancel: &'a dyn Fn() -> bool,
    last: (usize, String, f64),
    error: Option<ApiError>,
}

impl Writer<'_> {
    /// The ids of the nodes or the elements, in model order, from the base grid.
    fn ids(&self, nodes: bool) -> Vec<i32> {
        let (fields, key) = if nodes {
            (&self.grid.point_data, "NODE_ID")
        } else {
            (&self.grid.cell_data, "ELEMENT_ID")
        };
        match fields.iter().find(|f| f.name == key).map(|f| &f.values) {
            Some(fh_io::vtu::Values::I32(v)) => v.clone(),
            _ => Vec::new(),
        }
    }

    fn write_pvd(&self) -> Result<(), ApiError> {
        let text = write_pvd_units(&self.entries, self.units);
        atomic(self.run_dir, PVD_FILE, |w| {
            w.write_all(text.as_bytes()).map_err(|e| io(PVD_FILE, e))
        })
    }

    /// Write the result files of one increment. Each file is new and is never changed later.
    fn write_files(
        &mut self,
        step: usize,
        number: usize,
        last: bool,
        result: &StepResult,
    ) -> Result<(), ApiError> {
        let g = &mut self.grid;
        g.point_data
            .push(Field::f64("U", 3, flat(&result.displacement)));
        g.point_data
            .push(Field::f64("RF", 3, flat(&result.reaction)));
        if result.rotation.iter().flatten().any(|v| *v != 0.0) {
            g.point_data
                .push(Field::f64("UR", 3, flat(&result.rotation)));
        }
        g.cell_data.push(Field::f64(
            "S",
            6,
            result.element_stress.iter().flatten().copied().collect(),
        ));
        g.cell_data
            .push(Field::f64("MISES", 1, result.mises.clone()));
        let stem = result_name(step, number, last);
        let mut written = Ok(());
        if self.formats.vtu {
            let file = format!("{stem}.vtu");
            written = atomic(self.run_dir, &file, |w| {
                write_vtu_to(&self.grid, w).map_err(|e| ApiError::solver_output(e.to_string()))
            });
        }
        if written.is_ok() && self.formats.vtk {
            let file = format!("{stem}.vtk");
            let title = format!("fh step {} time {}", step + 1, result.time);
            written = atomic(self.run_dir, &file, |w| {
                write_vtk_to(&self.grid, &title, w)
                    .map_err(|e| ApiError::solver_output(e.to_string()))
            });
        }
        // The base grid stays for the next increment.
        let node_ids = self.ids(true);
        let element_ids = self.ids(false);
        let cell_types = self.grid.cell_types.clone();
        self.grid.point_data.truncate(self.n_point);
        self.grid.cell_data.truncate(self.n_cell);
        written?;
        if self.formats.text {
            let file = format!("{stem}.txt");
            atomic(self.run_dir, &file, |w| {
                write_text(
                    w,
                    step,
                    result,
                    self.units,
                    &node_ids,
                    &element_ids,
                    &cell_types,
                )
                .map_err(|e| io(&file, e))
            })?;
        }
        self.entries.push(PvdEntry {
            time: result.time,
            file: format!("{stem}.{}", primary(self.formats)),
        });
        Ok(())
    }

    fn restart_due(&self, inc: &Increment) -> bool {
        inc.last
            || inc.number.is_multiple_of(self.restart_every.max(1))
            || self
                .restart_minutes
                .is_some_and(|m| self.last_restart.elapsed().as_secs_f64() >= 60.0 * m)
    }

    fn write_restart(&mut self, inc: &Increment) -> Result<(), ApiError> {
        let part = (!inc.last).then_some(&inc.info);
        let restart = Restart {
            hash: self.hash,
            step: inc.step,
            number: inc.number,
            complete: inc.last,
            time: inc.time,
            lambda: inc.lambda,
            next_size: inc.next_size,
            displacement: inc.displacement.to_vec(),
            base: if inc.last {
                Vec::new()
            } else {
                inc.base.to_vec()
            },
            image: inc.states.image(),
            contact: inc.contact.image().ip,
            history: history_text(&self.entries, &self.incs, part),
        };
        restart
            .write(self.run_dir)
            .map_err(|e| io(RESTART_FILE, e))?;
        self.last_restart = Instant::now();
        (self.emit)(&format!("restart {RESTART_FILE}"));
        Ok(())
    }
}

fn method_name(info: &StepInfo) -> &'static str {
    match info.method {
        crate::Method::Pcg => "pcg",
        _ => "cholesky",
    }
}

impl Observer for Writer<'_> {
    fn estimate(&mut self, bytes: u64) {
        (self.emit)(&format!("estimate bytes={bytes}"));
    }

    fn progress(&mut self, step: usize, steps: usize, phase: &str, fraction: f64) {
        // At most about 20 lines for each phase.
        let (s, p, f) = &self.last;
        if *s == step
            && p == phase
            && fraction < 1.0
            && (fraction - f).abs() < 0.05
            && fraction > 0.0
        {
            return;
        }
        self.last = (step, phase.to_string(), fraction);
        (self.emit)(&format!(
            "progress step={step}/{steps} phase={phase} fraction={fraction:.3}"
        ));
    }

    fn cancelled(&mut self) -> bool {
        (self.cancel)()
    }

    fn cutback(&mut self, step: usize, number: usize, solves: usize) {
        self.incs.push(IncLine {
            step,
            number,
            iters: solves,
            ok: false,
        });
    }

    fn increment_done(&mut self, inc: &Increment, result: StepResult) -> Result<(), SolverError> {
        if inc.last {
            // Peak values of the step, for a person who reads the event lines.
            (self.emit)(&format!(
                "step {} max_u={:e} max_mises={:e} max_rf={:e} free_dofs={} method={} iterations={}",
                inc.step + 1,
                max_norm(&result.displacement),
                result.mises.iter().copied().fold(0.0, f64::max),
                max_norm(&result.reaction),
                inc.info.free_dofs,
                method_name(&inc.info),
                inc.info.iterations
            ));
        }
        self.incs.push(IncLine {
            step: inc.step,
            number: inc.number,
            iters: inc.solves,
            ok: true,
        });
        let mut run = || -> Result<(), ApiError> {
            self.write_files(inc.step, inc.number, inc.last, &result)?;
            if self.restart_due(inc) {
                self.write_restart(inc)?;
            }
            self.write_pvd()
        };
        run().map_err(|e| {
            let message = e.to_string();
            self.error = Some(e);
            SolverError::new(message)
        })
    }
}

/// The `fh.run` summary of a finished run (schema sections 3.5 and 3.8).
fn summary_toon(
    model: &Model,
    input: &Input,
    run_dir: &Path,
    incs: &[IncLine],
    summary: &Summary,
    wall: f64,
) -> Result<String, ApiError> {
    let step_name = |i: usize| {
        model.steps[i]
            .name
            .clone()
            .unwrap_or_else(|| format!("Step-{}", i + 1))
    };
    let steps = (0..model.steps.len())
        .map(|i| toon::RunStep {
            name: step_name(i),
            status: "converged".into(),
            reached: 1.0,
            increments: incs.iter().filter(|r| r.ok && r.step == i).count() as u64,
        })
        .collect();
    let increments = incs
        .iter()
        .map(|r| toon::Increment {
            step: step_name(r.step),
            inc: r.number as u64,
            iters: r.iters as u64,
            result: if r.ok {
                toon::IncResult::Ok
            } else {
                toon::IncResult::Cutback
            },
        })
        .collect();
    let diagnostics: Vec<toon::Diag> = summary
        .orphans
        .iter()
        .map(|(n, d)| toon::Diag {
            code: ORPHAN_CODE.into(),
            severity: toon::Severity::Warning,
            line: 0,
            message: format!("node N{n} dof {d} has no stiffness, it is fixed at zero"),
        })
        .collect();
    let run = toon::Run {
        header: toon::Header {
            source: input.name.to_string(),
            source_hash: format!("{:016x}", input.hash),
            units: input.units.into(),
            status: if diagnostics.is_empty() {
                toon::Status::Ok
            } else {
                toon::Status::Warning
            },
        },
        run_id: run_dir
            .file_name()
            .map_or_else(|| "run".into(), |n| n.to_string_lossy().to_string()),
        solver: "core".into(),
        results: PVD_FILE.into(),
        wall_s: wall,
        steps,
        increments,
        contact: None,
        failure: None,
        diagnostics,
        next: Vec::new(),
    };
    run.render(&toon::Opts::default())
        .map_err(|e| ApiError::solver_output(e.to_string()))
}

fn finish(
    model: &Model,
    input: &Input,
    writer: &mut Writer,
    run_dir: &Path,
    summary: &Summary,
    wall: f64,
) -> Result<RunReport, ApiError> {
    writer.write_pvd()?;
    let text = summary_toon(model, input, run_dir, &writer.incs, summary, wall)?;
    atomic(run_dir, SUMMARY_FILE, |w| {
        w.write_all(text.as_bytes())
            .map_err(|e| io(SUMMARY_FILE, e))
    })?;
    // Times and sizes go to the event stream, for a person or a benchmark that reads it.
    (writer.emit)(&format!(
        "stats dofs={} nnz={} pattern_builds={} estimate_bytes={} assemblies={} factorizations={} orphans={}",
        summary.total_dofs,
        summary.nnz,
        summary.pattern_builds,
        summary.estimate_bytes,
        summary.assemblies,
        summary.factorizations,
        summary.orphans.len()
    ));
    (writer.emit)(&format!(
        "timing assemble={:.6} factor={:.6} solve={:.6}",
        summary.assemble_seconds, summary.factor_seconds, summary.solve_seconds
    ));
    Ok(RunReport {
        pvd: run_dir.join(PVD_FILE),
        summary: text,
        validated: false,
        restart: Some(run_dir.join(RESTART_FILE)),
    })
}

/// Solve `model` and write the protocol files in `run_dir`. Event lines go to
/// `emit` (without the line end). `cancel` is polled during the run. With
/// `restart`, the run continues from the newest valid restart file.
pub fn run_model(
    model: &Model,
    input: &Input,
    run_dir: &Path,
    options: &Options,
    restart: bool,
    emit: &mut dyn FnMut(&str),
    cancel: &dyn Fn() -> bool,
) -> Result<RunReport, ApiError> {
    let clock = Instant::now();
    let hash = input.hash;
    fs::create_dir_all(run_dir).map_err(|e| io("run folder", e))?;
    let grid = base_grid(model)?;
    let (n_point, n_cell) = (grid.point_data.len(), grid.cell_data.len());
    let (mut entries, mut incs, mut resume) = (Vec::new(), Vec::new(), None);
    if restart {
        let r = read_latest(run_dir, hash).map_err(ApiError::solver_output)?;
        if r.step >= model.steps.len() {
            return Err(ApiError::solver_output(format!(
                "{RESTART_FILE}: more steps than the input file has"
            )));
        }
        clean_run_folder(run_dir, &r)?;
        let (e, l, part) = parse_history(&r.history)?;
        check_kept_results(run_dir, &e, options.formats)?;
        entries = e;
        incs = l;
        resume = Some(Resume {
            step: r.step,
            number: r.number,
            complete: r.complete,
            time: r.time,
            lambda: r.lambda,
            next_size: r.next_size,
            displacement: r.displacement,
            base: r.base,
            image: r.image,
            contact: r.contact,
            info: if r.complete { None } else { part },
        });
    } else {
        // A new run removes the temporary files of an earlier one.
        for e in fs::read_dir(run_dir)
            .map_err(|e| io("run folder", e))?
            .flatten()
        {
            if e.file_name().to_string_lossy().ends_with(".tmp") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    let mut writer = Writer {
        run_dir,
        formats: options.formats,
        hash,
        units: input.units,
        grid,
        n_point,
        n_cell,
        restart_every: options.restart_every,
        restart_minutes: options.restart_minutes,
        last_restart: Instant::now(),
        entries,
        incs,
        emit,
        cancel,
        last: (0, String::new(), 0.0),
        error: None,
    };
    let result = solve_from(model, options, &mut writer, resume.as_ref());
    let summary = match result {
        Ok(s) => s,
        Err(e) => {
            if let Some(api) = writer.error.take() {
                return Err(api);
            }
            if e.to_string() == CANCELLED {
                return Err(ApiError::cancelled());
            }
            return Err(ApiError::solver_output(e.to_string()));
        }
    };
    let wall = clock.elapsed().as_secs_f64();
    finish(model, input, &mut writer, run_dir, &summary, wall)
}

/// Write the results of a finished [`Solution`] in `run_dir`. Each step counts as one increment.
pub fn write_results(
    model: &Model,
    solution: &Solution,
    run_dir: &Path,
) -> Result<RunReport, ApiError> {
    fs::create_dir_all(run_dir).map_err(|e| io("run folder", e))?;
    let grid = base_grid(model)?;
    let (n_point, n_cell) = (grid.point_data.len(), grid.cell_data.len());
    let mut writer = Writer {
        run_dir,
        formats: Formats::default(),
        hash: 0,
        units: NO_UNITS,
        grid,
        n_point,
        n_cell,
        restart_every: 1,
        restart_minutes: None,
        last_restart: Instant::now(),
        entries: Vec::new(),
        incs: Vec::new(),
        emit: &mut |_| {},
        cancel: &|| false,
        last: (0, String::new(), 0.0),
        error: None,
    };
    for (i, step) in solution.steps.iter().enumerate() {
        let info = &solution.summary.steps[i];
        let number = info.increments.max(1);
        writer.write_files(i, number, true, step)?;
        writer.incs.push(IncLine {
            step: i,
            number,
            iters: info.newton_iterations.max(1),
            ok: true,
        });
    }
    let input = Input {
        name: "model",
        hash: 0,
        units: NO_UNITS,
    };
    finish(model, &input, &mut writer, run_dir, &solution.summary, 0.0)
}
