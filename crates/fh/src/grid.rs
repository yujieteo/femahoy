//! The built-in heat conduction and transmission line solvers of femahoy, in the process of `fh`.
//!
//! A grid case is a folder with `grid.toml`: the parts as boxes or as prisms of a polygon or a
//! circle with holes, the materials, the named face sets, the loads, the step and the probes
//! (see `fh guide grid`). A model of boxes is solved on the box grid; a model with a prism part
//! on a mesh of tetrahedra. [`run_with`] reads it, solves it with the crate `fh-fem` and writes (the reader and the result grid are in `fh_core::grid`,
//! which the browser solve of the viewer also uses):
//!
//! - `step-N.vtu` and `results.pvd` in the run folder: the temperature `NT` at each point, the
//!   heat flux `HFL` and the part number `part` of each cell, at each output time;
//! - `<run-dir>/grid/probes.txt`: the time and the temperature at each probe, 1 row for each
//!   output time;
//! - `<run-dir>/grid/energy.txt`: the heat flows of the energy balance at each output time;
//! - `<run-dir>/grid/log.txt`: the lines of progress and the warnings.
//!
//! A case with a `[line]` table is a transmission line cross-section (`fh_core::line`). Its run
//! writes the potential `V1`... and the current density `J1`... of the triangles to `step-1.vtu`,
//! the per-length parameters to `<run-dir>/grid/line.txt` and, with a length, the S parameters to
//! `<run-dir>/grid/line.s2Np` (Touchstone).
//!
//! The solver is femahoy's own code, so it starts no program. A cancel and the time limit stop it
//! between 2 iterations, time steps or frequencies.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use fh_core::grid::{frame_grid, is_line, probe_head, probe_row, read};
use fh_core::line;
use fh_core::vtu::PvdEntry;
use fh_fem::heat::{self, Events, Frame, Mesh};

use crate::ccx::{prepare_run_dir, CancelToken, PVD_FILE};
use crate::vtu::{write_pvd, write_vtu};

/// The file of a grid case.
pub const CASE_FILE: &str = "grid.toml";
/// The folder of the text results in a run folder.
pub const WORK: &str = "grid";
pub const PROBES: &str = "probes.txt";
pub const ENERGY: &str = "energy.txt";
/// The table of the per-length parameters of a line case.
pub const LINE: &str = "line.txt";

/// True when `dir` is a grid case: it has `grid.toml`.
pub fn is_case(dir: &Path) -> bool {
    dir.join(CASE_FILE).is_file()
}

/// Solve the case `case` in the folder `run_dir`.
pub fn run(case: &Path, run_dir: &Path) -> Result<(), String> {
    run_with(case, run_dir, None, &CancelToken::new(), &mut |_| {})
}

/// Solve, and stop when `cancel` is set or after `timeout`. `on_line` gets each line of progress
/// and each warning.
pub fn run_with(
    case: &Path,
    run_dir: &Path,
    timeout: Option<Duration>,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let file = case.join(CASE_FILE);
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    if is_line(&text) {
        return run_line(case, &file, &text, run_dir, timeout, cancel, on_line);
    }
    let (model, units) = read(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let run_dir = prepare_run_dir(run_dir, &file)?;
    let work = run_dir.join(WORK);
    crate::getdp::copy_case(case, &work, false)?;
    let mut out = Output {
        dir: run_dir.clone(),
        mesh: None,
        entries: Vec::new(),
        probes: probe_head(&model),
        energy: "# time source flux convection fixed stored imbalance\n".into(),
        log: String::new(),
        on_line,
        cancel,
        end: timeout.map(|t| Instant::now() + t),
    };
    let result = heat::solve(&model, &mut out);
    let summary = match result {
        Ok(s) => s,
        Err(_) if !cancel.is_cancelled() && out.end.is_some_and(|end| Instant::now() >= end) => {
            return Err(format!(
                "the time limit {} s stopped the solve",
                timeout.map_or(0.0, |t| t.as_secs_f64())
            ))
        }
        Err(e) if cancel.is_cancelled() => return Err(format!("{}: {e}", crate::ccx::CANCELLED)),
        Err(e) => return Err(e),
    };
    out.line(&format!(
        "{} cells, {} points, {} unknowns, largest energy imbalance {:.1e}",
        summary.cells, summary.points, summary.unknowns, summary.imbalance
    ));
    for w in &summary.warnings {
        out.line(&format!("warning: {w}"));
    }
    let write = |name: &str, text: &str| {
        fs::write(work.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write(PROBES, &out.probes)?;
    write(ENERGY, &out.energy)?;
    write("log.txt", &out.log)?;
    let pvd = write_pvd(&out.entries, &units, false);
    let tmp = run_dir.join(format!("{PVD_FILE}.tmp"));
    fs::write(&tmp, pvd)
        .and_then(|()| fs::rename(&tmp, run_dir.join(PVD_FILE)))
        .map_err(|e| format!("{PVD_FILE}: {e}"))
}

/// Solve the transmission line case `text` (of the file `file` in `case`).
fn run_line(
    case: &Path,
    file: &Path,
    text: &str,
    run_dir: &Path,
    timeout: Option<Duration>,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let model = line::read(text).map_err(|e| format!("{}: {e}", file.display()))?;
    let run_dir = prepare_run_dir(run_dir, file)?;
    let work = run_dir.join(WORK);
    crate::getdp::copy_case(case, &work, false)?;
    struct Progress<'a> {
        log: String,
        on_line: &'a mut dyn FnMut(&str),
        cancel: &'a CancelToken,
        end: Option<Instant>,
    }
    impl Progress<'_> {
        fn line(&mut self, text: &str) {
            self.log.push_str(text);
            self.log.push('\n');
            (self.on_line)(text);
        }
    }
    impl fh_fem::line::Events for Progress<'_> {
        fn cancelled(&self) -> bool {
            self.cancel.is_cancelled() || self.end.is_some_and(|end| Instant::now() >= end)
        }
        fn progress(&mut self, text: &str) {
            self.line(text);
        }
    }
    let mut out = Progress {
        log: String::new(),
        on_line,
        cancel,
        end: timeout.map(|t| Instant::now() + t),
    };
    let solution = match fh_fem::line::solve(&model, &mut out) {
        Ok(s) => s,
        Err(_) if !cancel.is_cancelled() && out.end.is_some_and(|end| Instant::now() >= end) => {
            return Err(format!(
                "the time limit {} s stopped the solve",
                timeout.map_or(0.0, |t| t.as_secs_f64())
            ))
        }
        Err(e) if cancel.is_cancelled() => return Err(format!("{}: {e}", crate::ccx::CANCELLED)),
        Err(e) => return Err(e),
    };
    for w in &solution.warnings {
        out.line(&format!("warning: {w}"));
    }
    let write = |name: &str, text: &str| {
        fs::write(work.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write(LINE, &line::table_text(&model, &solution))?;
    if let Some(ts) = line::touchstone_text(&model, &solution) {
        write(&line::touchstone_name(&model), &ts)?;
    }
    write("log.txt", &out.log)?;
    let file = "step-1.vtu";
    fs::write(
        run_dir.join(file),
        write_vtu(&line::solution_grid(&solution))?,
    )
    .map_err(|e| format!("{file}: {e}"))?;
    let entries = [PvdEntry {
        time: 0.0,
        file: file.into(),
    }];
    let tmp = run_dir.join(format!("{PVD_FILE}.tmp"));
    fs::write(&tmp, write_pvd(&entries, "SI-m", false))
        .and_then(|()| fs::rename(&tmp, run_dir.join(PVD_FILE)))
        .map_err(|e| format!("{PVD_FILE}: {e}"))
}

/// The writer of the results while the solve runs.
struct Output<'a> {
    dir: std::path::PathBuf,
    mesh: Option<Mesh>,
    entries: Vec<PvdEntry>,
    probes: String,
    energy: String,
    log: String,
    on_line: &'a mut dyn FnMut(&str),
    cancel: &'a CancelToken,
    end: Option<Instant>,
}

impl Output<'_> {
    fn line(&mut self, text: &str) {
        self.log.push_str(text);
        self.log.push('\n');
        (self.on_line)(text);
    }
}

impl Events for Output<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.end.is_some_and(|end| Instant::now() >= end)
    }

    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.mesh = Some(mesh.clone());
        Ok(())
    }

    fn progress(&mut self, text: &str) {
        self.line(text);
    }

    fn frame(&mut self, frame: &Frame) -> Result<(), String> {
        let mesh = self.mesh.as_ref().expect("the mesh comes before a frame");
        let grid = frame_grid(mesh, frame);
        let file = format!("step-{}.vtu", self.entries.len() + 1);
        let tmp = self.dir.join(format!("{file}.tmp"));
        fs::write(&tmp, write_vtu(&grid)?)
            .and_then(|()| fs::rename(&tmp, self.dir.join(&file)))
            .map_err(|e| format!("{file}: {e}"))?;
        self.entries.push(PvdEntry {
            time: frame.time,
            file,
        });
        self.probes.push_str(&probe_row(frame));
        let e = frame.energy;
        let row: Vec<String> = [
            frame.time,
            e.source,
            e.flux,
            e.convection,
            e.fixed,
            e.stored,
            e.imbalance(),
        ]
        .iter()
        .map(|v| format!("{v:.10e}"))
        .collect();
        self.energy.push_str(&row.join(" "));
        self.energy.push('\n');
        Ok(())
    }
}
