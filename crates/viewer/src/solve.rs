//! The solve of a `grid.toml` case in the browser, with no `fh run`: the built-in grid solver
//! (`fh-fem`) runs in a worker of the page (`web` module), and each output goes to the page as
//! the lists of [`results::Results::receive`]. `fh run` uses the same reader, result grid and
//! probe table (`fh_core::grid`), so the same case gives the same probes. A case with a `[line]`
//! table is a transmission line (`fh_core::line`): its answer is the table of `grid/line.txt`.

use fh_core::grid::{
    em_frame_grid, frame_grid, is_line, modes_row, probe_head, probe_row, read, sparams_head,
    sparams_row, Case, MODES_HEAD,
};
use fh_core::line;
use fh_core::vtu::PvdEntry;
use fh_fem::em;
use fh_fem::heat::{self, Events, Frame, Limit, Mesh};

use crate::results::Loader;

/// The largest grid that the browser solves. Reason: wasm32 addresses at most 4 GiB. In Chrome, a
/// solve of 1,030,301 points grew the memory of the worker to 1,821 MiB (steady) and 2,121 MiB
/// (transient). So 1,000,000 points use about 2 GiB, and half of the 4 GiB stays free for the
/// growth of a list and for a browser that gives less. The limit of a mesh of tetrahedra has the
/// same ratio to the native limit (1,000,000 / 4,000,000 of 400,000). Reason: the native direct
/// solve of 336,141 points used 2.7 GB, so 400,000 points do not fit in the worker, and there is
/// no measurement of the direct solve in wasm32.
pub const BROWSER: Limits = Limits {
    heat: Limit {
        points: 1_000_000,
        tet_points: 100_000,
        memory: MEMORY,
    },
    em: em::Limit {
        points: 40_000,
        memory: MEMORY,
        direct: 25_000,
    },
};

/// The case `text` that the browser solves (heat or electromagnetics) and its unit system. A
/// static or a homogenize step gets an error that names `fh run`.
pub fn read_case(text: &str) -> Result<(Case, String), String> {
    match read(text)? {
        (Case::Elastic(_) | Case::Homogenize(_), _) => Err("the browser solves only heat steps \
             (steady or transient) and electromagnetic steps (frequency or modes). Solve a static \
             or a homogenize step with fh run"
            .into()),
        case => Ok(case),
    }
}

/// The largest mesh of a line case in the browser: a quarter of the native limit, the ratio of the
/// limits of the box grid. Reason: the native 2D solve of 321,295 points used a factor of 200 MB
/// at each frequency, so 250,000 points fit in the worker with a large margin; there is no
/// measurement of the line solve in wasm32.
pub const BROWSER_LINE_POINTS: usize = 250_000;

const MEMORY: &str = "the 4 GiB memory of wasm32 in a browser";

/// The largest models of a solve: heat, and electromagnetics. Reason for the electromagnetic
/// limits: natively, the direct factor of a mesh of 31,415 points has 84,931,624 complex entries
/// (1.4 GB), so 25,000 points use about 1 GB of the 4 GiB of wasm32. The iterative solve of
/// 39,818 points used 1.16 GB. There is no measurement in wasm32.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub heat: Limit,
    pub em: em::Limit,
}

/// Solve the case `text` (the text of `grid.toml`). `on_line` gets each line of progress and
/// each warning, and `on_step` the lists of each output. The answer is the probe table, the
/// same text as `grid/probes.txt` of `fh run`; for electromagnetics, the S-parameter table
/// (`grid/sparams.txt`) or the mode table (`grid/modes.txt`).
pub fn solve(
    text: &str,
    limits: Limits,
    on_line: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(Vec<Vec<u8>>),
) -> Result<String, String> {
    if is_line(text) {
        return solve_line(text, on_line, on_step);
    }
    let (case, units) = read_case(text).map_err(|e| format!("grid.toml: {e}"))?;
    let model = match case {
        Case::Em(model) => return solve_em(&model, &units, limits.em, on_line, on_step),
        Case::Heat(model) => model,
        Case::Elastic(_) | Case::Homogenize(_) => unreachable!("read_case refuses them"),
    };
    let limit = limits.heat;
    let mut out = Output {
        mesh: None,
        loader: Loader::stream(&units),
        probes: probe_head(&model),
        on_line,
        on_step,
    };
    let summary = heat::solve_within(&model, limit, &mut out).map_err(|e| {
        if e.contains(limit.memory) {
            format!(
                "{e}, or solve the case with fh run (up to {} points of a box grid and {} of a \
                 mesh of tetrahedra)",
                heat::LAPTOP.points,
                heat::LAPTOP.tet_points
            )
        } else {
            e
        }
    })?;
    (out.on_line)(&format!(
        "{} cells, {} points, {} unknowns, largest energy imbalance {:.1e}",
        summary.cells, summary.points, summary.unknowns, summary.imbalance
    ));
    for w in &summary.warnings {
        (out.on_line)(&format!("warning: {w}"));
    }
    Ok(out.probes)
}

/// Solve the line case `text`: 1 output with the potential and the current density, and the
/// answer is the table of the per-length parameters.
fn solve_line(
    text: &str,
    on_line: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(Vec<Vec<u8>>),
) -> Result<String, String> {
    let mut model = line::read(text).map_err(|e| format!("grid.toml: {e}"))?;
    model.max_points = BROWSER_LINE_POINTS;
    struct Progress<'a>(&'a mut dyn FnMut(&str));
    impl fh_fem::line::Events for Progress<'_> {
        fn progress(&mut self, text: &str) {
            (self.0)(text);
        }
    }
    let solution = fh_fem::line::solve(&model, &mut Progress(on_line)).map_err(|e| {
        if e.contains("more than") {
            format!(
                "{e}. The browser solves up to {BROWSER_LINE_POINTS} points; fh run solves up to \
                 {}",
                line::LAPTOP_POINTS
            )
        } else {
            e
        }
    })?;
    for w in &solution.warnings {
        on_line(&format!("warning: {w}"));
    }
    let mut loader = Loader::stream("SI-m");
    let entry = PvdEntry {
        time: 0.0,
        file: "step-1".into(),
    };
    on_step(loader.push_and_send(entry, line::solution_grid(&solution))?);
    Ok(line::table_text(&model, &solution))
}

/// Solve an electromagnetic case.
fn solve_em(
    model: &em::Model,
    units: &str,
    limit: em::Limit,
    on_line: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(Vec<Vec<u8>>),
) -> Result<String, String> {
    let modes = matches!(model.step, em::Step::Modes { .. });
    let mut out = EmOutput {
        mesh: None,
        loader: Loader::stream(units),
        table: if modes {
            MODES_HEAD.to_owned()
        } else {
            sparams_head(model.ports.len())
        },
        count: 0,
        on_line,
        on_step,
    };
    let summary = em::solve_within(model, limit, &mut out).map_err(|e| {
        if e.contains(limit.memory) {
            format!(
                "{e}, or solve the case with fh run (up to {} points)",
                em::LAPTOP.points
            )
        } else {
            e
        }
    })?;
    (out.on_line)(&format!(
        "{} cells, {} points, {} unknowns",
        summary.cells, summary.points, summary.unknowns
    ));
    for w in &summary.warnings {
        (out.on_line)(&format!("warning: {w}"));
    }
    Ok(out.table)
}

struct EmOutput<'a> {
    mesh: Option<Mesh>,
    loader: Loader,
    table: String,
    count: usize,
    on_line: &'a mut dyn FnMut(&str),
    on_step: &'a mut dyn FnMut(Vec<Vec<u8>>),
}

impl em::Events for EmOutput<'_> {
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.mesh = Some(mesh.clone());
        Ok(())
    }

    fn progress(&mut self, text: &str) {
        (self.on_line)(text);
    }

    fn frame(&mut self, frame: &em::Frame) -> Result<(), String> {
        self.count += 1;
        if frame.q.is_some() {
            self.table.push_str(&modes_row(self.count, frame));
        } else {
            self.table.push_str(&sparams_row(frame));
        }
        if frame.fields.is_empty() {
            return Ok(());
        }
        let mesh = self.mesh.as_ref().ok_or("the mesh comes before a frame")?;
        let entry = PvdEntry {
            time: frame.frequency,
            file: format!("step-{}", self.count),
        };
        let lists = self
            .loader
            .push_and_send(entry, em_frame_grid(mesh, frame))?;
        (self.on_step)(lists);
        Ok(())
    }
}

struct Output<'a> {
    mesh: Option<Mesh>,
    loader: Loader,
    probes: String,
    on_line: &'a mut dyn FnMut(&str),
    on_step: &'a mut dyn FnMut(Vec<Vec<u8>>),
}

impl Events for Output<'_> {
    fn mesh(&mut self, mesh: &Mesh) -> Result<(), String> {
        self.mesh = Some(mesh.clone());
        Ok(())
    }

    fn progress(&mut self, text: &str) {
        (self.on_line)(text);
    }

    fn frame(&mut self, frame: &Frame) -> Result<(), String> {
        let mesh = self.mesh.as_ref().ok_or("the mesh comes before a frame")?;
        let entry = PvdEntry {
            time: frame.time,
            file: format!("step-{}", self.probes.lines().count()),
        };
        let lists = self.loader.push_and_send(entry, frame_grid(mesh, frame))?;
        (self.on_step)(lists);
        self.probes.push_str(&probe_row(frame));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::Results;

    const WALL: &str = include_str!("../../../validation/cases/grid-layered-wall/wall/grid.toml");
    const COAX: &str = include_str!("../../../validation/cases/line-coax/coax/grid.toml");

    /// A line case gives the page the potential and the table of the per-length parameters.
    #[test]
    fn the_page_gets_the_fields_and_the_table_of_a_line() {
        let mut results = Results::default();
        let coarse = COAX.replace("max_cell = 4e-5", "max_cell = 2e-4");
        let table = solve(&coarse, BROWSER, &mut |_| {}, &mut |s| {
            results.receive(&s[..]).unwrap()
        })
        .unwrap();
        assert!(table.contains("# f R1_1 L1_1"), "{table}");
        assert_eq!(results.steps.len(), 1);
        assert!(results.steps[0].field("V1").is_some());
    }

    /// The page gets each output with its temperature field, and a larger grid than the limit
    /// is refused before the solve, with the limit and the memory in the text.
    #[test]
    fn the_page_gets_the_results_and_a_large_grid_is_refused() {
        let mut results = Results::default();
        let mut lines = Vec::new();
        let probes = solve(WALL, BROWSER, &mut |l| lines.push(l.to_owned()), &mut |s| {
            results.receive(&s[..]).unwrap()
        })
        .unwrap();
        assert_eq!(probes.lines().count(), 2, "{probes}");
        assert_eq!(results.steps.len(), 1);
        assert_eq!(results.units, "SI-m");
        assert!(results.steps[0].field("NT").is_some());
        assert!(lines.iter().any(|l| l.contains("unknowns")), "{lines:?}");
        let small = Limits {
            heat: Limit {
                points: 10,
                ..BROWSER.heat
            },
            em: BROWSER.em,
        };
        let e = solve(WALL, small, &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(
            e.contains("more than 10 points") && e.contains("wasm32") && e.contains("fh run"),
            "{e}"
        );
    }

    /// A static step gets an error that names fh run, not a solve.
    #[test]
    fn a_static_step_is_refused_with_fh_run_in_the_text() {
        const BLOCK: &str =
            include_str!("../../../validation/cases/grid-constrained-block/block/grid.toml");
        let e = solve(BLOCK, BROWSER, &mut |_| {}, &mut |_| panic!("no output")).unwrap_err();
        assert!(e.contains("heat steps") && e.contains("fh run"), "{e}");
    }

    const GUIDE: &str =
        include_str!("../../../validation/cases/grid-waveguide-te10/guide/grid.toml");

    /// An electromagnetic case gives the S-parameter table and the field of each port.
    #[test]
    fn the_page_gets_the_s_parameters_and_the_fields_of_a_waveguide() {
        let mut results = Results::default();
        let table = solve(GUIDE, BROWSER, &mut |_| {}, &mut |s| {
            results.receive(&s[..]).unwrap()
        })
        .unwrap();
        assert!(table.starts_with("# frequency S11_re"), "{table}");
        assert_eq!(table.lines().count(), 2, "{table}");
        assert_eq!(results.steps.len(), 1);
        assert!(results.steps[0].field("E1_mag").is_some());
        assert!(results.steps[0].field("E2_re magnitude").is_some());
    }
}
