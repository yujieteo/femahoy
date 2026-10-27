//! The built-in heat conduction, linear elasticity and transmission line solvers of femahoy, in
//! the process of `fh`.
//!
//! A grid case is a folder with `grid.toml`: the parts as boxes or as prisms of a polygon or a
//! circle with holes, the materials, the named face sets, the loads, the step, the probes and the
//! surfaces (see `fh guide grid`). A model of boxes is solved on the box grid, and a heat model
//! with a prism part on a mesh of tetrahedra. [`run_with`] reads it, solves it with the crate
//! `fh-fem` and writes, for a heat step (steady or transient), with the reader and the result
//! grid of `fh_core::grid`, which the browser solve of the viewer also uses:
//!
//! - `step-N.vtu` and `results.pvd` in the run folder: the temperature `NT` at each point, the
//!   heat flux `HFL` and the part number `part` of each cell, at each output time;
//! - `<run-dir>/grid/probes.txt`: the time and the temperature at each probe, 1 row for each
//!   output time;
//! - `<run-dir>/grid/energy.txt`: the heat flows of the energy balance at each output time;
//! - `<run-dir>/grid/log.txt`: the lines of progress and the warnings.
//!
//! For a static step:
//!
//! - `step-1.vtu` and `results.pvd`: the displacement `U`, the stress `S`, the strain `E`, the
//!   von Mises stress `MISES` and the principal stresses `SP` at each point (the mean of the
//!   cells around it), the temperature `NT` with a thermal load, and `part`;
//! - `<run-dir>/grid/probes.txt`: 1 row with the columns `NAME.ux`, `NAME.uy`, `NAME.uz`,
//!   `NAME.sxx` ... `NAME.sxz`, `NAME.mises` (and `NAME.t` with a thermal load) of each probe;
//! - `<run-dir>/grid/surfaces.txt`: 1 row with the shape of each surface: `NAME.range`,
//!   `NAME.flatness`, the curvatures (`NAME.kxx`, `NAME.kyy`, `NAME.kxy` for a z face), the
//!   principal curvatures `NAME.k1` and `NAME.k2`, and `NAME.shape` (convex, concave or flat);
//! - `<run-dir>/grid/reactions.txt`: 1 row with `NAME.fx`, `NAME.fy`, `NAME.fz` of each fixed
//!   face set;
//! - `<run-dir>/grid/stress.txt`: the largest von Mises stress of each part, its point, and
//!   whether the point is singular, with the largest value away from the singular points;
//! - `<run-dir>/grid/log.txt`.
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

use fh_core::grid::{frame_grid, is_line, probe_head, probe_row, read, Case};
use fh_core::line;
use fh_core::vtu::{Field, Grid as VtuGrid, PvdEntry, VTK_HEXAHEDRON};
use fh_fem::elastic::{self, Solution};
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

pub const SURFACES: &str = "surfaces.txt";
pub const REACTIONS: &str = "reactions.txt";
pub const STRESS: &str = "stress.txt";

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
        tables: Vec::new(),
        probes: String::new(),
        energy: "# time source flux convection fixed stored imbalance\n".into(),
        log: String::new(),
        on_line,
        cancel,
        end: timeout.map(|t| Instant::now() + t),
    };
    let result = match &model {
        Case::Heat(model) => {
            out.probes = probe_head(model);
            heat::solve(model, &mut out).map(|s| {
                (
                    format!(
                        "{} cells, {} points, {} unknowns, largest energy imbalance {:.1e}",
                        s.cells, s.points, s.unknowns, s.imbalance
                    ),
                    s.warnings,
                )
            })
        }
        Case::Elastic(model) => elastic::solve(model, &mut Static(&mut out, model)).map(|s| {
            (
                format!(
                    "{} cells, {} points, {} unknowns, force imbalance {:.1e}",
                    s.cells, s.points, s.unknowns, s.imbalance
                ),
                s.warnings,
            )
        }),
    };
    let (summary, warnings) = match result {
        Ok(s) => s,
        Err(e)
            if e == "cancelled"
                && !cancel.is_cancelled()
                && out.end.is_some_and(|end| Instant::now() >= end) =>
        {
            return Err(format!(
                "the time limit {} s stopped the solve",
                timeout.map_or(0.0, |t| t.as_secs_f64())
            ))
        }
        Err(e) if cancel.is_cancelled() => return Err(format!("{}: {e}", crate::ccx::CANCELLED)),
        Err(e) => return Err(e),
    };
    out.line(&summary);
    for w in &warnings {
        out.line(&format!("warning: {w}"));
    }
    let write = |name: &str, text: &str| {
        fs::write(work.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    write(PROBES, &out.probes)?;
    if matches!(model, Case::Heat(_)) {
        write(ENERGY, &out.energy)?;
    }
    for (name, text) in &out.tables {
        write(name, text)?;
    }
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
        Err(e)
            if e == "cancelled"
                && !cancel.is_cancelled()
                && out.end.is_some_and(|end| Instant::now() >= end) =>
        {
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
    /// The tables of a static step after `probes.txt`.
    tables: Vec<(&'static str, String)>,
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

    fn cancelled(&self) -> bool {
        self.cancel.is_cancelled() || self.end.is_some_and(|end| Instant::now() >= end)
    }

    /// Write `step-N.vtu` with the fields, at `time`.
    fn vtu(
        &mut self,
        time: f64,
        point_data: Vec<Field>,
        cell_data: Vec<Field>,
    ) -> Result<(), String> {
        let mesh = self.mesh.as_ref().expect("the mesh comes before a result");
        let mut cell_data = cell_data;
        cell_data.push(Field::f64(
            "part",
            1,
            mesh.part.iter().map(|&p| f64::from(p) + 1.0).collect(),
        ));
        let grid = VtuGrid {
            points: mesh.points.clone(),
            cell_types: vec![VTK_HEXAHEDRON; mesh.len()],
            cells: mesh.cells.chunks_exact(8).map(|c| c.to_vec()).collect(),
            point_data,
            cell_data,
        };
        self.step(time, &grid)
    }

    /// Write `step-N.vtu` with `grid`, at `time`.
    fn step(&mut self, time: f64, grid: &VtuGrid) -> Result<(), String> {
        let file = format!("step-{}.vtu", self.entries.len() + 1);
        let tmp = self.dir.join(format!("{file}.tmp"));
        fs::write(&tmp, write_vtu(grid)?)
            .and_then(|()| fs::rename(&tmp, self.dir.join(&file)))
            .map_err(|e| format!("{file}: {e}"))?;
        self.entries.push(PvdEntry { time, file });
        Ok(())
    }
}

/// A header line and 1 row.
fn row(columns: &[(String, String)]) -> String {
    let (names, values): (Vec<&str>, Vec<&str>) = columns
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .unzip();
    format!("# {}\n{}\n", names.join(" "), values.join(" "))
}

fn real(v: f64) -> String {
    format!("{v:.10e}")
}

impl Events for Output<'_> {
    fn cancelled(&self) -> bool {
        Output::cancelled(self)
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
        self.step(frame.time, &grid)?;
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
        .into_iter()
        .map(real)
        .collect();
        self.energy.push_str(&row.join(" "));
        self.energy.push('\n');
        Ok(())
    }
}

/// The writer of a static step: the output and the model, for the names.
struct Static<'a, 'b>(&'a mut Output<'b>, &'a elastic::Model);

impl elastic::Events for Static<'_, '_> {
    fn cancelled(&self) -> bool {
        self.0.cancelled()
    }

    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.0.mesh = Some(mesh.clone());
        Ok(())
    }

    fn progress(&mut self, text: &str) {
        self.0.line(text);
    }

    fn solution(&mut self, s: &Solution) -> Result<(), String> {
        let model = self.1;
        let flat = |v: &[[f64; 6]]| v.iter().flatten().copied().collect::<Vec<_>>();
        let mut fields = vec![
            Field::f64("U", 3, s.displacement.iter().flatten().copied().collect()),
            Field::f64("S", 6, flat(&s.stress)),
            Field::f64("E", 6, flat(&s.strain)),
            Field::f64("MISES", 1, s.mises.clone()),
            Field::f64("SP", 3, s.principal.iter().flatten().copied().collect()),
        ];
        if let Some(t) = &s.temperature {
            fields.push(Field::f64("NT", 1, t.clone()));
        }
        self.0.vtu(1.0, fields, Vec::new())?;
        let mut columns = Vec::new();
        for (p, v) in model.probes.iter().zip(&s.probes) {
            let names = [
                "ux", "uy", "uz", "sxx", "syy", "szz", "sxy", "syz", "sxz", "mises",
            ];
            let values = v.displacement.iter().chain(&v.stress).chain([&v.mises]);
            for (n, &x) in names.iter().zip(values) {
                columns.push((format!("{}.{n}", p.name), real(x)));
            }
            if s.temperature.is_some() {
                columns.push((format!("{}.t", p.name), real(v.temperature)));
            }
        }
        self.0.probes = row(&columns);
        let mut columns = Vec::new();
        for shape in &s.surfaces {
            let n = &shape.name;
            let [a, b] = [[1, 2], [0, 2], [0, 1]][shape.axis].map(|d| ["x", "y", "z"][d]);
            columns.push((format!("{n}.range"), real(shape.range)));
            columns.push((format!("{n}.flatness"), real(shape.flatness)));
            for (k, v) in [format!("{a}{a}"), format!("{b}{b}"), format!("{a}{b}")]
                .iter()
                .zip(shape.curvature)
            {
                columns.push((format!("{n}.k{k}"), real(v)));
            }
            columns.push((format!("{n}.k1"), real(shape.principal[0])));
            columns.push((format!("{n}.k2"), real(shape.principal[1])));
            columns.push((format!("{n}.shape"), shape.sign().to_owned()));
        }
        if !columns.is_empty() {
            self.0.tables.push((SURFACES, row(&columns)));
        }
        let columns: Vec<_> = s
            .reactions
            .iter()
            .flat_map(|r| {
                ["fx", "fy", "fz"]
                    .iter()
                    .zip(r.force)
                    .map(|(c, f)| (format!("{}.{c}", r.name), real(f)))
                    .collect::<Vec<_>>()
            })
            .collect();
        self.0.tables.push((REACTIONS, row(&columns)));
        let mut stress = String::from(
            "# part mises x y z singular regular_mises regular_x regular_y regular_z\n",
        );
        for peak in &s.peaks {
            let regular = peak.regular.map_or(
                ["nan".to_owned(), "nan".into(), "nan".into(), "nan".into()],
                |(m, p)| [real(m), real(p[0]), real(p[1]), real(p[2])],
            );
            let singular = match peak.singular {
                None => "no",
                Some(elastic::Singular::ReEntrant) => "re-entrant",
                Some(elastic::Singular::MaterialEdge) => "material-edge",
                Some(elastic::Singular::SupportEdge) => "support-edge",
            };
            stress.push_str(&format!(
                "{} {} {} {} {} {singular} {}\n",
                model.parts[peak.part].name,
                real(peak.mises),
                real(peak.point[0]),
                real(peak.point[1]),
                real(peak.point[2]),
                regular.join(" ")
            ));
        }
        self.0.tables.push((STRESS, stress));
        Ok(())
    }
}
