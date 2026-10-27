//! The built-in solvers of femahoy (heat conduction, linear elasticity, unit cells, transmission
//! lines and frequency-domain electromagnetics), in the process of `fh`.
//!
//! A grid case is a folder with `grid.toml`: the parts as boxes or as prisms of a polygon or a
//! circle with holes, the materials, the named face sets, the loads, the step, the probes and the
//! surfaces (see `fh guide grid`). A model of boxes is solved on the box grid; a model with a
//! prism part, and each electromagnetic model, on a mesh of tetrahedra (10-node for a static
//! step). [`run_with`] reads it, solves it with the crate `fh-fem` and writes, for a heat step
//! (steady or transient), with the reader and the result grid of `fh_core::grid`, which the
//! browser solve of the viewer also uses:
//!
//! - `step-N.vtu` and `results.pvd` in the run folder: for heat, the temperature `NT` at each
//!   point, the heat flux `HFL` and the part number `part` of each cell, at each output time;
//!   for electromagnetics, E of each port excitation (or of each mode) at each cell, with the
//!   frequency as the time;
//! - `<run-dir>/grid/probes.txt`: the time and the temperature at each probe, 1 row for each
//!   output time;
//! - `<run-dir>/grid/energy.txt`: the heat flows of the energy balance at each output time;
//! - `<run-dir>/grid/sparams.txt`, `ports.txt` and `sparams.sNp`: the S-parameters, the
//!   propagation constant and the impedance of each port, and the Touchstone file, 1 row for
//!   each frequency; `<run-dir>/grid/modes.txt`: the frequency and Q of each mode;
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
//! For a homogenize step (a periodic unit cell):
//!
//! - `step-1.vtu` and `results.pvd`: the mesh of the cell with `part`;
//! - `<run-dir>/grid/homogenized.txt`: 1 row with the volume fraction of each material, the
//!   effective conductivity tensor, stiffness tensor, engineering constants, expansion, density
//!   and specific heat;
//! - `<run-dir>/grid/material.toml`: the effective material as a `[material.homogenized]` table
//!   for the `grid.toml` of a larger model;
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

use fh_core::grid::{
    em_frame_grid, frame_grid, is_line, modes_row, ports_head, ports_row, probe_head, probe_row,
    read, sparams_head, sparams_row, touchstone_head, touchstone_rows, vtk_type, Case, MODES_HEAD,
};
use fh_core::line;
use fh_core::vtu::{Field, Grid as VtuGrid, PvdEntry};
use fh_fem::elastic::{self, Solution};
use fh_fem::em;
use fh_fem::heat::{self, Events, Frame, Mesh};
use fh_fem::homog::{self, diagonal, engineering, orthotropic, Effective, COUPLING};

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
pub const SPARAMS: &str = "sparams.txt";
pub const PORTS: &str = "ports.txt";
pub const MODES: &str = "modes.txt";

pub const SURFACES: &str = "surfaces.txt";
pub const REACTIONS: &str = "reactions.txt";
pub const STRESS: &str = "stress.txt";
/// The effective material of a unit cell, as a table and as a `grid.toml` material.
pub const HOMOGENIZED: &str = "homogenized.txt";
pub const MATERIAL: &str = "material.toml";

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
    let (case_model, units) = read(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    let run_dir = prepare_run_dir(run_dir, &file)?;
    let work = run_dir.join(WORK);
    crate::getdp::copy_case(case, &work, false)?;
    let model = match case_model {
        Case::Em(model) => {
            return run_em(&model, &units, &run_dir, timeout, cancel, on_line);
        }
        model => model,
    };
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
        Case::Homogenize(model) => homog::solve(model, &mut out).and_then(|eff| {
            let (material, warnings) = material_text(&eff);
            for line in tensor_lines(&eff) {
                out.line(&line);
            }
            out.tables.push((HOMOGENIZED, table_text(model, &eff)));
            out.tables.push((MATERIAL, material));
            out.mesh = Some(eff.mesh.clone());
            out.vtu(1.0, Vec::new(), Vec::new())?;
            Ok((
                format!(
                    "{} cells, {} points, {} unknowns",
                    eff.mesh.len(),
                    eff.mesh.points.len(),
                    eff.unknowns
                ),
                warnings,
            ))
        }),
        Case::Em(_) => unreachable!("run_em solves an electromagnetic case"),
    };
    let (summary, warnings) = stopped(result, cancel, out.end, timeout)?;
    out.line(&summary);
    for w in &warnings {
        out.line(&format!("warning: {w}"));
    }
    let write = |name: &str, text: &str| {
        fs::write(work.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    if !matches!(model, Case::Homogenize(_)) {
        write(PROBES, &out.probes)?;
    }
    if matches!(model, Case::Heat(_)) {
        write(ENERGY, &out.energy)?;
    }
    for (name, text) in &out.tables {
        write(name, text)?;
    }
    write("log.txt", &out.log)?;
    finish_pvd(&run_dir, &out.entries, &units)
}

/// The summary of a solve, or the error of a solve that a cancel or the time limit stopped.
fn stopped<T>(
    result: Result<T, String>,
    cancel: &CancelToken,
    end: Option<Instant>,
    timeout: Option<Duration>,
) -> Result<T, String> {
    match result {
        Ok(s) => Ok(s),
        Err(e)
            if e == "cancelled"
                && !cancel.is_cancelled()
                && end.is_some_and(|end| Instant::now() >= end) =>
        {
            Err(format!(
                "the time limit {} s stopped the solve",
                timeout.map_or(0.0, |t| t.as_secs_f64())
            ))
        }
        Err(e) if cancel.is_cancelled() => Err(format!("{}: {e}", crate::ccx::CANCELLED)),
        Err(e) => Err(e),
    }
}

fn finish_pvd(run_dir: &Path, entries: &[PvdEntry], units: &str) -> Result<(), String> {
    let pvd = write_pvd(entries, units, false);
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
    let result = fh_fem::line::solve(&model, &mut out);
    let solution = stopped(result, cancel, out.end, timeout)?;
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

/// Write the grid `grid` as the next `step-N.vtu` of the run folder `dir`.
fn write_step(
    dir: &Path,
    entries: &mut Vec<PvdEntry>,
    time: f64,
    grid: &VtuGrid,
) -> Result<(), String> {
    let file = format!("step-{}.vtu", entries.len() + 1);
    let tmp = dir.join(format!("{file}.tmp"));
    fs::write(&tmp, write_vtu(grid)?)
        .and_then(|()| fs::rename(&tmp, dir.join(&file)))
        .map_err(|e| format!("{file}: {e}"))?;
    entries.push(PvdEntry { time, file });
    Ok(())
}

/// Solve an electromagnetic case and write its tables and fields.
fn run_em(
    model: &em::Model,
    units: &str,
    run_dir: &Path,
    timeout: Option<Duration>,
    cancel: &CancelToken,
    on_line: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let work = run_dir.join(WORK);
    let ports = model.ports.len();
    let mut out = EmOutput {
        dir: run_dir.to_path_buf(),
        mesh: None,
        entries: Vec::new(),
        sparams: sparams_head(ports),
        ports: ports_head(ports),
        touchstone: touchstone_head(ports),
        modes: MODES_HEAD.to_owned(),
        count: 0,
        log: String::new(),
        on_line,
        cancel,
        end: timeout.map(|t| Instant::now() + t),
    };
    let result = em::solve(model, &mut out);
    let summary = stopped(result, cancel, out.end, timeout)?;
    out.line(&format!(
        "{} cells, {} points, {} unknowns",
        summary.cells, summary.points, summary.unknowns
    ));
    for w in &summary.warnings {
        out.line(&format!("warning: {w}"));
    }
    let write = |name: &str, text: &str| {
        fs::write(work.join(name), text).map_err(|e| format!("{name}: {e}"))
    };
    match model.step {
        em::Step::Frequency { .. } => {
            write(SPARAMS, &out.sparams)?;
            write(PORTS, &out.ports)?;
            write(&format!("sparams.s{ports}p"), &out.touchstone)?;
        }
        em::Step::Modes { .. } => write(MODES, &out.modes)?,
    }
    write("log.txt", &out.log)?;
    finish_pvd(run_dir, &out.entries, units)
}

/// The writer of the results of an electromagnetic solve while it runs.
struct EmOutput<'a> {
    dir: std::path::PathBuf,
    mesh: Option<Mesh>,
    entries: Vec<PvdEntry>,
    sparams: String,
    ports: String,
    touchstone: String,
    modes: String,
    /// The frames so far.
    count: usize,
    log: String,
    on_line: &'a mut dyn FnMut(&str),
    cancel: &'a CancelToken,
    end: Option<Instant>,
}

impl EmOutput<'_> {
    fn line(&mut self, text: &str) {
        self.log.push_str(text);
        self.log.push('\n');
        (self.on_line)(text);
    }
}

impl em::Events for EmOutput<'_> {
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

    fn frame(&mut self, frame: &em::Frame) -> Result<(), String> {
        self.count += 1;
        if frame.q.is_some() {
            self.modes.push_str(&modes_row(self.count, frame));
            self.line(&format!(
                "mode {}: {:.6e} Hz, Q {:.4e}",
                self.count,
                frame.frequency,
                frame.q.unwrap_or(f64::INFINITY)
            ));
        } else {
            self.sparams.push_str(&sparams_row(frame));
            self.ports.push_str(&ports_row(frame));
            self.touchstone.push_str(&touchstone_rows(frame));
            let n = frame.ports.len();
            let s: Vec<String> = (0..n)
                .map(|i| format!("|S{}1| {:.4}", i + 1, frame.s[i * n].abs()))
                .collect();
            self.line(&format!("{:.6e} Hz: {}", frame.frequency, s.join(", ")));
        }
        if !frame.fields.is_empty() {
            let mesh = self.mesh.as_ref().expect("the mesh comes before a frame");
            let grid = em_frame_grid(mesh, frame);
            write_step(&self.dir, &mut self.entries, frame.frequency, &grid)?;
        }
        Ok(())
    }
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
            cell_types: vec![vtk_type(mesh.kind); mesh.len()],
            cells: (0..mesh.len()).map(|e| mesh.cell(e).to_vec()).collect(),
            point_data,
            cell_data,
        };
        write_step(&self.dir, &mut self.entries, time, &grid)
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
        write_step(&self.dir, &mut self.entries, frame.time, &grid)?;
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

impl homog::Events for Output<'_> {
    fn cancelled(&self) -> bool {
        Output::cancelled(self)
    }

    fn progress(&mut self, text: &str) {
        self.line(text);
    }
}

/// The names of the 6 components, in the order of the stiffness of `fh_fem::hex`.
const VOIGT: [&str; 6] = ["xx", "yy", "zz", "yz", "xz", "xy"];

/// The row of `homogenized.txt`.
fn table_text(model: &homog::Model, eff: &Effective) -> String {
    let mut columns = Vec::new();
    for (m, f) in model.materials.iter().zip(&eff.fraction) {
        columns.push((format!("fraction.{}", m.name), real(*f)));
    }
    if let Some(k) = eff.conductivity {
        for (c, [i, j]) in VOIGT
            .iter()
            .zip([[0, 0], [1, 1], [2, 2], [1, 2], [0, 2], [0, 1]])
        {
            columns.push((format!("k{c}"), real(k[i][j])));
        }
    }
    if let Some(c) = eff.stiffness {
        for (i, row) in c.iter().enumerate() {
            for (j, v) in row.iter().enumerate().skip(i) {
                columns.push((format!("c{}{}", i + 1, j + 1), real(*v)));
            }
        }
        if let Some(o) = engineering(&c) {
            for (n, v) in ["ex", "ey", "ez"].iter().zip(o.e) {
                columns.push((n.to_string(), real(v)));
            }
            for (n, v) in ["nuxy", "nuxz", "nuyz"].iter().zip(o.nu) {
                columns.push((n.to_string(), real(v)));
            }
            for (n, v) in ["gxy", "gxz", "gyz"].iter().zip(o.g) {
                columns.push((n.to_string(), real(v)));
            }
        }
    }
    if let Some(a) = eff.expansion {
        for (c, v) in VOIGT.iter().zip(a) {
            columns.push((format!("alpha{c}"), real(v)));
        }
    }
    if let Some(r) = eff.density {
        columns.push(("density".into(), real(r)));
    }
    if let Some(c) = eff.specific_heat {
        columns.push(("specific_heat".into(), real(c)));
    }
    row(&columns)
}

/// The tensors of the cell as lines of the log.
fn tensor_lines(eff: &Effective) -> Vec<String> {
    let fmt = |v: &[f64]| {
        v.iter()
            .map(|x| format!("{x:12.5e}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut lines = Vec::new();
    if let Some(k) = eff.conductivity {
        lines.push("effective conductivity (x, y, z):".to_owned());
        lines.extend(k.iter().map(|r| format!("  {}", fmt(r))));
    }
    if let Some(c) = eff.stiffness {
        lines.push(
            "effective stiffness (xx, yy, zz, yz, xz, xy, engineering shear strains):".to_owned(),
        );
        lines.extend(c.iter().map(|r| format!("  {}", fmt(r))));
    }
    if let Some(a) = eff.expansion {
        lines.push(format!("effective expansion (xx ... xy): {}", fmt(&a)));
    }
    lines
}

/// The effective material as a `grid.toml` table, and the warnings of the properties that a
/// `grid.toml` material cannot hold.
fn material_text(eff: &Effective) -> (String, Vec<String>) {
    let list = |v: &[f64]| {
        let items: Vec<String> = v.iter().map(|x| format!("{x:.6e}")).collect();
        format!("[{}]", items.join(", "))
    };
    let mut warnings = Vec::new();
    let mut text = String::from(
        "# The homogenized material of the unit cell (fh run). Copy the table into the grid.toml \
         of a larger\n# model, and give it the name of the material there. The material axes are \
         x, y and z of the cell.\n[material.homogenized]\n",
    );
    if let Some(k) = eff.conductivity {
        if diagonal(&k) {
            text.push_str(&format!(
                "conductivity = {}\n",
                list(&[k[0][0], k[1][1], k[2][2]])
            ));
        } else {
            warnings.push(format!(
                "the effective conductivity has terms that couple 2 axes ({HOMOGENIZED}), and a \
                 grid.toml material has 1 value for each axis: {MATERIAL} has no conductivity"
            ));
        }
    }
    if let (Some(c), Some(a)) = (eff.stiffness, eff.expansion) {
        let shear = a[3..].iter().map(|v| v.abs()).fold(0.0, f64::max);
        let normal = a[..3].iter().map(|v| v.abs()).fold(0.0, f64::max);
        match engineering(&c) {
            Some(o) if orthotropic(&c) && shear <= COUPLING * normal.max(f64::MIN_POSITIVE) => {
                text.push_str(&format!("youngs_modulus = {}\n", list(&o.e)));
                text.push_str(&format!("poisson_ratio = {}\n", list(&o.nu)));
                text.push_str(&format!("shear_modulus = {}\n", list(&o.g)));
                text.push_str(&format!("expansion = {}\n", list(&a[..3])));
            }
            _ => warnings.push(format!(
                "the effective stiffness or expansion couples a normal and a shear component \
                 ({HOMOGENIZED}): the cell is not orthotropic in x, y and z, and a grid.toml \
                 material is. {MATERIAL} has no elastic constants"
            )),
        }
    }
    if let Some(r) = eff.density {
        text.push_str(&format!("density = {r:.6e}\n"));
        if eff.specific_heat.is_none() {
            text.push_str(
                "# A steady or transient heat step also needs specific_heat. Add it, or remove density.\n",
            );
        }
    }
    if let Some(c) = eff.specific_heat {
        text.push_str(&format!("specific_heat = {c:.6e}\n"));
    }
    (text, warnings)
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
