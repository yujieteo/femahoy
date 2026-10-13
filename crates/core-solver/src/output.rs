//! Results files for the subprocess protocol: for each step a text table, a VTU file and a
//! legacy VTK file (each can be switched off with `Options::formats`), a PVD collection, a
//! restart file and the `fh.run` TOON summary.
//!
//! The writer takes one increment at a time from the solver and keeps only the
//! base grid in memory. Every file is written under a temporary name and renamed.

use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use fh_inp::Model;
use fh_io::vtk::write_vtk_to;
use fh_io::vtu::{
    write_pvd_units, write_vtu_to, Field, Grid, PvdEntry, VTK_HEXAHEDRON, VTK_QUADRATIC_TETRA,
};
use fh_solver_api::{ApiError, RunReport, Solver, PVD_FILE, SUMMARY_FILE};

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
            0,
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
        let kind = Kind::parse(&el.element_type)
            .ok_or_else(|| ApiError(format!("element {}: unsupported type", el.id)))?;
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
                    .ok_or_else(|| ApiError(format!("node {n} not found")))
            })
            .collect::<Result<_, _>>()?;
        grid.cells.push(cell);
    }
    let ids = |what: &str, v: &[u32]| -> Result<Vec<i32>, ApiError> {
        v.iter()
            .map(|&i| {
                i32::try_from(i).map_err(|_| {
                    ApiError(format!(
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

/// Numbers of one finished step.
#[derive(Debug, Clone, PartialEq)]
struct StepLine {
    free_dofs: usize,
    method: String,
    iterations: usize,
    residual: f64,
    max_u: f64,
    max_mises: f64,
    max_rf: f64,
}

impl StepLine {
    fn text(&self) -> String {
        format!(
            "{} {} {} {:e} {:e} {:e} {:e}",
            self.free_dofs,
            self.method,
            self.iterations,
            self.residual,
            self.max_u,
            self.max_mises,
            self.max_rf
        )
    }

    fn parse(text: &str) -> Option<StepLine> {
        let mut w = text.split_whitespace();
        Some(StepLine {
            free_dofs: w.next()?.parse().ok()?,
            method: w.next()?.to_string(),
            iterations: w.next()?.parse().ok()?,
            residual: w.next()?.parse().ok()?,
            max_u: w.next()?.parse().ok()?,
            max_mises: w.next()?.parse().ok()?,
            max_rf: w.next()?.parse().ok()?,
        })
    }
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
    ApiError(format!("{what}: {e}"))
}

/// Write a file under a temporary name and rename it. A stop never leaves half a file.
fn atomic(
    dir: &Path,
    file: &str,
    body: impl FnOnce(&mut BufWriter<fs::File>) -> Result<(), ApiError>,
) -> Result<(), ApiError> {
    let tmp = dir.join(format!("{file}.tmp"));
    let mut w = BufWriter::new(fs::File::create(&tmp).map_err(|e| io(file, e))?);
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
fn history_text(entries: &[PvdEntry], lines: &[StepLine], part: Option<&StepInfo>) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&format!("entry {:e} {}\n", e.time, e.file));
    }
    for l in lines {
        s.push_str(&format!("step {}\n", l.text()));
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

/// The inverse of [`history_text`].
fn parse_history(
    text: &str,
) -> Result<(Vec<PvdEntry>, Vec<StepLine>, Option<StepInfo>), ApiError> {
    let bad = || ApiError(format!("{RESTART_FILE}: bad history line"));
    let (mut entries, mut lines, mut part) = (Vec::new(), Vec::new(), None);
    for l in text.lines() {
        if let Some(r) = l.strip_prefix("entry ") {
            let (t, f) = r.split_once(' ').ok_or_else(bad)?;
            entries.push(PvdEntry {
                time: t.parse().map_err(|_| bad())?,
                file: f.to_string(),
            });
        } else if let Some(r) = l.strip_prefix("step ") {
            lines.push(StepLine::parse(r).ok_or_else(bad)?);
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
    Ok((entries, lines, part))
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

struct Writer<'a> {
    run_dir: &'a Path,
    formats: Formats,
    hash: u64,
    grid: Grid,
    n_point: usize,
    n_cell: usize,
    restart_every: usize,
    restart_minutes: Option<f64>,
    last_restart: Instant,
    entries: Vec<PvdEntry>,
    lines: Vec<StepLine>,
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
        let text = write_pvd_units(&self.entries, "unspecified");
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
        info: &StepInfo,
    ) -> Result<(), ApiError> {
        let g = &mut self.grid;
        g.point_data
            .push(Field::f64("U", 3, flat(&result.displacement)));
        g.point_data.push(Field::f64("RF", 3, flat(&result.reaction)));
        if result.rotation.iter().flatten().any(|v| *v != 0.0) {
            g.point_data.push(Field::f64("UR", 3, flat(&result.rotation)));
        }
        g.cell_data.push(Field::f64(
            "S",
            6,
            result.element_stress.iter().flatten().copied().collect(),
        ));
        g.cell_data.push(Field::f64("MISES", 1, result.mises.clone()));
        let stem = result_name(step, number, last);
        let mut written = Ok(());
        if self.formats.vtu {
            let file = format!("{stem}.vtu");
            written = atomic(self.run_dir, &file, |w| {
                write_vtu_to(&self.grid, w).map_err(|e| ApiError(e.to_string()))
            });
        }
        if written.is_ok() && self.formats.vtk {
            let file = format!("{stem}.vtk");
            let title = format!("fh step {} time {}", step + 1, result.time);
            written = atomic(self.run_dir, &file, |w| {
                write_vtk_to(&self.grid, &title, w).map_err(|e| ApiError(e.to_string()))
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
                write_text(w, step, result, &node_ids, &element_ids, &cell_types)
                    .map_err(|e| io(&file, e))
            })?;
        }
        self.entries.push(PvdEntry {
            time: result.time,
            file: format!("{stem}.{}", primary(self.formats)),
        });
        if last {
            self.lines.push(StepLine {
                free_dofs: info.free_dofs,
                method: method_name(info).into(),
                iterations: info.iterations,
                residual: info.residual,
                max_u: max_norm(&result.displacement),
                max_mises: result.mises.iter().copied().fold(0.0, f64::max),
                max_rf: max_norm(&result.reaction),
            });
        }
        Ok(())
    }

    fn restart_due(&self, inc: &Increment) -> bool {
        inc.last
            || inc.number % self.restart_every.max(1) == 0
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
            base: if inc.last { Vec::new() } else { inc.base.to_vec() },
            image: inc.states.image(),
            contact: Vec::new(),
            history: history_text(&self.entries, &self.lines, part),
        };
        restart.write(self.run_dir).map_err(|e| io(RESTART_FILE, e))?;
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

    fn increment_done(&mut self, inc: &Increment, result: StepResult) -> Result<(), SolverError> {
        let mut run = || -> Result<(), ApiError> {
            self.write_files(inc.step, inc.number, inc.last, &result, &inc.info)?;
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

fn summary_toon(model: &Model, lines: &[StepLine], summary: &Summary) -> String {
    let mut s = String::from("schema: fh.run\nversion: 1.0.0\nstatus: ok\n");
    s.push_str(&format!(
        "nodes: {}\nelements: {}\ndofs: {}\nnnz: {}\nestimate_bytes: {}\npattern_builds: {}\n",
        model.nodes.len(),
        model.elements.len(),
        summary.total_dofs,
        summary.nnz,
        summary.estimate_bytes,
        summary.pattern_builds
    ));
    s.push_str(&format!(
        "orphans[{}]{{node,dof}}:\n",
        summary.orphans.len()
    ));
    for (n, d) in &summary.orphans {
        s.push_str(&format!("  {n},{d}\n"));
    }
    s.push_str(&format!(
        "steps[{}]{{step,free_dofs,method,iterations,residual,max_u,max_mises,max_rf}}:\n",
        lines.len()
    ));
    for (i, l) in lines.iter().enumerate() {
        s.push_str(&format!(
            "  {},{},{},{},{:e},{:e},{:e},{:e}\n",
            i + 1,
            l.free_dofs,
            l.method,
            l.iterations,
            l.residual,
            l.max_u,
            l.max_mises,
            l.max_rf
        ));
    }
    s
}

fn finish(
    model: &Model,
    writer: &mut Writer,
    run_dir: &Path,
    summary: &Summary,
) -> Result<RunReport, ApiError> {
    writer.write_pvd()?;
    let text = summary_toon(model, &writer.lines, summary);
    atomic(run_dir, SUMMARY_FILE, |w| {
        w.write_all(text.as_bytes()).map_err(|e| io(SUMMARY_FILE, e))
    })?;
    // Times change from run to run, so they go to the event stream and not to the files.
    (writer.emit)(&format!(
        "timing assemble={:.3} factor={:.3} solve={:.3}",
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
    hash: u64,
    run_dir: &Path,
    options: &Options,
    restart: bool,
    emit: &mut dyn FnMut(&str),
    cancel: &dyn Fn() -> bool,
) -> Result<RunReport, ApiError> {
    fs::create_dir_all(run_dir).map_err(|e| io("run folder", e))?;
    let grid = base_grid(model)?;
    let (n_point, n_cell) = (grid.point_data.len(), grid.cell_data.len());
    let (mut entries, mut lines, mut resume) = (Vec::new(), Vec::new(), None);
    if restart {
        let r = read_latest(run_dir, hash).map_err(ApiError)?;
        if r.step >= model.steps.len() {
            return Err(ApiError(format!(
                "{RESTART_FILE}: more steps than the input file has"
            )));
        }
        clean_run_folder(run_dir, &r)?;
        let (e, l, part) = parse_history(&r.history)?;
        entries = e;
        lines = l;
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
            info: if r.complete { None } else { part },
        });
    } else {
        // A new run removes the temporary files of an earlier one.
        for e in fs::read_dir(run_dir).map_err(|e| io("run folder", e))?.flatten() {
            if e.file_name().to_string_lossy().ends_with(".tmp") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
    let mut writer = Writer {
        run_dir,
        formats: options.formats,
        hash,
        grid,
        n_point,
        n_cell,
        restart_every: options.restart_every,
        restart_minutes: options.restart_minutes,
        last_restart: Instant::now(),
        entries,
        lines,
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
            return Err(ApiError(e.to_string()));
        }
    };
    finish(model, &mut writer, run_dir, &summary)
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
        grid,
        n_point,
        n_cell,
        restart_every: 1,
        restart_minutes: None,
        last_restart: Instant::now(),
        entries: Vec::new(),
        lines: Vec::new(),
        emit: &mut |_| {},
        cancel: &|| false,
        last: (0, String::new(), 0.0),
        error: None,
    };
    for (i, step) in solution.steps.iter().enumerate() {
        let info = &solution.summary.steps[i];
        writer.write_files(i, info.increments.max(1), true, step, info)?;
    }
    finish(model, &mut writer, run_dir, &solution.summary)
}
