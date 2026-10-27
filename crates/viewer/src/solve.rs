//! The solve of a `grid.toml` case in the browser, with no `fh run`: the built-in grid solver
//! (`fh-fem`) runs in a worker of the page (`web` module), and each output goes to the page as
//! the lists of [`results::Results::receive`]. `fh run` uses the same reader, result grid and
//! probe table (`fh_core::grid`), so the same case gives the same probes.

use fh_core::grid::{frame_grid, probe_head, probe_row, read};
use fh_core::vtu::PvdEntry;
use fh_fem::heat::{self, Events, Frame, Limit, Mesh};

use crate::results::Loader;

/// The largest grid that the browser solves. Reason: wasm32 addresses at most 4 GiB. In Chrome, a
/// solve of 1,030,301 points grew the memory of the worker to 1,821 MiB (steady) and 2,121 MiB
/// (transient). So 1,000,000 points use about 2 GiB, and half of the 4 GiB stays free for the
/// growth of a list and for a browser that gives less.
pub const BROWSER: Limit = Limit {
    points: 1_000_000,
    memory: "the 4 GiB memory of wasm32 in a browser",
};

/// Solve the case `text` (the text of `grid.toml`). `on_line` gets each line of progress and
/// each warning, and `on_step` the lists of each output. The answer is the probe table, the
/// same text as `grid/probes.txt` of `fh run`.
pub fn solve(
    text: &str,
    limit: Limit,
    on_line: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(Vec<Vec<u8>>),
) -> Result<String, String> {
    let (model, units) = read(text).map_err(|e| format!("grid.toml: {e}"))?;
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
                "{e}, or solve the case with fh run (up to {} points)",
                heat::LAPTOP.points
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
        let small = Limit {
            points: 10,
            memory: BROWSER.memory,
        };
        let e = solve(WALL, small, &mut |_| {}, &mut |_| {}).unwrap_err();
        assert!(
            e.contains("more than 10 points") && e.contains("wasm32") && e.contains("fh run"),
            "{e}"
        );
    }
}
