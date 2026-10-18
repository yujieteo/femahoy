//! `fh.run` (schema section 3.5). The subprocess solver summary (section 3.8)
//! uses the same type.

use crate::common::{finish, start, tbl, Diag, Header, Next};
use crate::wrap::{Doc, Error, Opts};

#[derive(Clone, Debug)]
pub struct RunStep {
    pub name: String,
    pub status: String,
    pub reached: f64,
    pub increments: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncResult {
    Ok,
    Cutback,
}

impl IncResult {
    fn as_str(self) -> &'static str {
        match self {
            IncResult::Ok => "ok",
            IncResult::Cutback => "cutback",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Increment {
    pub step: String,
    pub inc: u64,
    pub iters: u64,
    pub result: IncResult,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub header: Header,
    pub run_id: String,
    pub solver: String,
    pub results: String,
    pub wall_s: f64,
    pub steps: Vec<RunStep>,
    /// All increments in solve order. The output ranks them.
    pub increments: Vec<Increment>,
    pub diagnostics: Vec<Diag>,
    pub next: Vec<Next>,
}

impl Run {
    pub fn render(&self, opts: &Opts) -> Result<String, Error> {
        let iterations = self
            .increments
            .iter()
            .try_fold(0u64, |total, i| total.checked_add(i.iters))
            .ok_or_else(|| Error("iteration count overflow".into()))?;
        let cutbacks = self
            .increments
            .iter()
            .filter(|i| i.result == IncResult::Cutback)
            .count();
        let totals = vec![
            ("steps", self.steps.len().into()),
            ("increments", self.increments.len().into()),
            ("cutbacks", cutbacks.into()),
            ("iterations", iterations.into()),
            ("wall_s", self.wall_s.into()),
        ];
        let doc: Doc = start("fh.run", &self.header, opts)
            .scalar("run_id", self.run_id.as_str())
            .scalar("solver", self.solver.as_str())
            .scalar("results", self.results.as_str())
            .block("totals", totals);
        let doc = tbl(
            doc,
            "steps",
            ["name", "status", "reached", "increments"],
            "no steps",
            &self.steps,
            opts.rows,
            |s| {
                [
                    s.name.as_str().into(),
                    s.status.as_str().into(),
                    s.reached.into(),
                    s.increments.into(),
                ]
            },
        );

        // Cutback increments first, then the others with the last one first.
        let (bad, good): (Vec<&Increment>, Vec<&Increment>) = self
            .increments
            .iter()
            .partition(|i| i.result != IncResult::Ok);
        let ranked = bad.into_iter().chain(good.into_iter().rev());
        let doc = tbl(
            doc,
            "increments",
            ["step", "inc", "iters", "result"],
            "no increments",
            ranked,
            opts.rows,
            |i| {
                [
                    i.step.as_str().into(),
                    i.inc.into(),
                    i.iters.into(),
                    i.result.as_str().into(),
                ]
            },
        );

        finish(doc, &self.diagnostics, &self.next, opts).encode(opts)
    }
}
