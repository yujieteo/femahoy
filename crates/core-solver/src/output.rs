//! Results files for the subprocess protocol: one VTU file for each step, a
//! PVD collection, a restart file and the `fh.run` TOON summary.
//!
//! The writer takes one step at a time from the solver and keeps only the
//! base grid in memory.

use std::fs;
use std::io::BufWriter;
use std::path::Path;

use fh_inp::Model;
use fh_io::vtu::{
    write_pvd_units, write_vtu_to, Field, Grid, PvdEntry, VTK_HEXAHEDRON, VTK_QUADRATIC_TETRA,
};
use fh_solver_api::{ApiError, RunReport, Solver, PVD_FILE, SUMMARY_FILE};

use crate::element::Kind;
use crate::system::{solve_from, step_times, CANCELLED};
use crate::{Observer, Options, Solution, SolverError, StepInfo, StepResult, Summary};

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

fn io(what: &str, e: std::io::Error) -> ApiError {
    ApiError(format!("{what}: {e}"))
}

/// Read the restart file. Returns the lines of the finished steps.
fn read_restart(dir: &Path, hash: u64) -> Result<Vec<StepLine>, ApiError> {
    let text = fs::read_to_string(dir.join(RESTART_FILE))
        .map_err(|e| ApiError(format!("{RESTART_FILE}: {e}")))?;
    let mut lines = text.lines();
    if lines.next() != Some("fh-restart 1") {
        return Err(ApiError(format!("{RESTART_FILE}: unknown format")));
    }
    let want = format!("input-hash {hash:016x}");
    if lines.next() != Some(want.as_str()) {
        return Err(ApiError(format!(
            "{RESTART_FILE}: the input file is not the one of the earlier run"
        )));
    }
    let done: usize = lines
        .next()
        .and_then(|l| l.strip_prefix("steps-done "))
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| ApiError(format!("{RESTART_FILE}: bad steps-done line")))?;
    let steps: Vec<StepLine> = lines
        .filter_map(|l| l.strip_prefix("step "))
        .map(|l| {
            StepLine::parse(l).ok_or_else(|| ApiError(format!("{RESTART_FILE}: bad step line")))
        })
        .collect::<Result<_, _>>()?;
    if steps.len() != done {
        return Err(ApiError(format!("{RESTART_FILE}: step lines do not match")));
    }
    Ok(steps)
}

struct Writer<'a> {
    run_dir: &'a Path,
    hash: u64,
    grid: Grid,
    n_point: usize,
    n_cell: usize,
    times: Vec<f64>,
    entries: Vec<PvdEntry>,
    lines: Vec<StepLine>,
    emit: &'a mut dyn FnMut(&str),
    cancel: &'a dyn Fn() -> bool,
    last: (usize, String, f64),
    error: Option<ApiError>,
}

impl Writer<'_> {
    fn write_pvd(&self) -> Result<(), ApiError> {
        fs::write(
            self.run_dir.join(PVD_FILE),
            write_pvd_units(&self.entries, "unspecified"),
        )
        .map_err(|e| io(PVD_FILE, e))
    }

    fn write_restart(&self) -> Result<(), ApiError> {
        let mut text = format!(
            "fh-restart 1\ninput-hash {:016x}\nsteps-done {}\n",
            self.hash,
            self.lines.len()
        );
        for l in &self.lines {
            text.push_str(&format!("step {}\n", l.text()));
        }
        // Write a new file and rename it, so that a stop never leaves half a file.
        let tmp = self.run_dir.join(format!("{RESTART_FILE}.tmp"));
        fs::write(&tmp, text).map_err(|e| io(RESTART_FILE, e))?;
        fs::rename(&tmp, self.run_dir.join(RESTART_FILE)).map_err(|e| io(RESTART_FILE, e))
    }

    fn write_step(
        &mut self,
        index: usize,
        step: &StepResult,
        info: &StepInfo,
    ) -> Result<(), ApiError> {
        let g = &mut self.grid;
        g.point_data
            .push(Field::f64("U", 3, flat(&step.displacement)));
        g.point_data.push(Field::f64("RF", 3, flat(&step.reaction)));
        if step.rotation.iter().flatten().any(|v| *v != 0.0) {
            g.point_data.push(Field::f64("UR", 3, flat(&step.rotation)));
        }
        g.cell_data.push(Field::f64(
            "S",
            6,
            step.element_stress.iter().flatten().copied().collect(),
        ));
        g.cell_data.push(Field::f64("MISES", 1, step.mises.clone()));
        let file = format!("step-{}.vtu", index + 1);
        let out = fs::File::create(self.run_dir.join(&file)).map_err(|e| io(&file, e))?;
        let mut w = BufWriter::new(out);
        let written = write_vtu_to(&self.grid, &mut w).map_err(|e| ApiError(e.to_string()));
        // The base grid stays for the next step.
        self.grid.point_data.truncate(self.n_point);
        self.grid.cell_data.truncate(self.n_cell);
        written?;
        std::io::Write::flush(&mut w).map_err(|e| io(&file, e))?;
        self.entries.push(PvdEntry {
            time: step.time,
            file,
        });
        self.lines.push(StepLine {
            free_dofs: info.free_dofs,
            method: method_name(info).into(),
            iterations: info.iterations,
            residual: info.residual,
            max_u: max_norm(&step.displacement),
            max_mises: step.mises.iter().copied().fold(0.0, f64::max),
            max_rf: max_norm(&step.reaction),
        });
        self.write_pvd()?;
        self.write_restart()?;
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

    fn step_done(
        &mut self,
        index: usize,
        result: StepResult,
        info: &StepInfo,
    ) -> Result<(), SolverError> {
        self.write_step(index, &result, info).map_err(|e| {
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

/// Solve `model` and write the protocol files in `run_dir`. Event lines go to
/// `emit` (without the line end). `cancel` is polled during the run. With
/// `restart`, the steps that the restart file lists are not solved again.
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
    let times = step_times(model);
    let lines = if restart {
        read_restart(run_dir, hash)?
    } else {
        Vec::new()
    };
    if lines.len() > times.len() {
        return Err(ApiError(format!(
            "{RESTART_FILE}: more steps than the input file has"
        )));
    }
    let mut entries = Vec::new();
    for (i, &time) in times.iter().enumerate().take(lines.len()) {
        let file = format!("step-{}.vtu", i + 1);
        if !run_dir.join(&file).is_file() {
            return Err(ApiError(format!("restart: {file} is missing")));
        }
        entries.push(PvdEntry { time, file });
    }
    let skip = lines.len();
    let mut writer = Writer {
        run_dir,
        hash,
        grid,
        n_point,
        n_cell,
        times,
        entries,
        lines,
        emit,
        cancel,
        last: (0, String::new(), 0.0),
        error: None,
    };
    let _ = &writer.times;
    let result = solve_from(model, options, &mut writer, skip);
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
    writer.write_pvd()?;
    let text = summary_toon(model, &writer.lines, &summary);
    fs::write(run_dir.join(SUMMARY_FILE), &text).map_err(|e| io(SUMMARY_FILE, e))?;
    Ok(RunReport {
        pvd: run_dir.join(PVD_FILE),
        summary: text,
        validated: false,
        restart: Some(run_dir.join(RESTART_FILE)),
    })
}

/// Write the results of a finished [`Solution`] in `run_dir`.
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
        hash: 0,
        grid,
        n_point,
        n_cell,
        times: step_times(model),
        entries: Vec::new(),
        lines: Vec::new(),
        emit: &mut |_| {},
        cancel: &|| false,
        last: (0, String::new(), 0.0),
        error: None,
    };
    for (i, step) in solution.steps.iter().enumerate() {
        let info = &solution.summary.steps[i];
        writer.write_step(i, step, info)?;
    }
    writer.write_pvd()?;
    let text = summary_toon(model, &writer.lines, &solution.summary);
    fs::write(run_dir.join(SUMMARY_FILE), &text).map_err(|e| io(SUMMARY_FILE, e))?;
    Ok(RunReport {
        pvd: run_dir.join(PVD_FILE),
        summary: text,
        validated: false,
        restart: Some(run_dir.join(RESTART_FILE)),
    })
}
