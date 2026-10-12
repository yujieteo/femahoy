//! `fh.run` (schema section 3.5). The subprocess solver summary (section 3.8)
//! uses the same type.

use crate::common::{finish, start, tbl, Diag, Header, Next};
use crate::wrap::{Doc, Error, Opts};

/// The `worst` table lists at most this many nodes.
const MAX_WORST: usize = 5;

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
    Failed,
}

impl IncResult {
    fn as_str(self) -> &'static str {
        match self {
            IncResult::Ok => "ok",
            IncResult::Cutback => "cutback",
            IncResult::Failed => "failed",
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
pub struct ContactRow {
    pub pair: String,
    pub closed: u64,
    pub max_pen: f64,
    pub force: f64,
}

#[derive(Clone, Debug)]
pub struct Residual {
    pub iter: u64,
    pub force: f64,
    pub disp: f64,
    pub contact_changes: u64,
}

#[derive(Clone, Debug)]
pub struct Worst {
    pub label: String,
    pub set: String,
    pub force: f64,
}

/// Extra tables for `status: failed`.
#[derive(Clone, Debug)]
pub struct Failure {
    /// The last failed increment.
    pub residuals: Vec<Residual>,
    /// Nodes with the largest residual force, largest first.
    pub worst: Vec<Worst>,
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
    /// `None` when the model has no contact.
    pub contact: Option<Vec<ContactRow>>,
    pub failure: Option<Failure>,
    pub diagnostics: Vec<Diag>,
    pub next: Vec<Next>,
}

impl Run {
    pub fn render(&self, opts: &Opts) -> Result<String, Error> {
        let cutbacks = self
            .increments
            .iter()
            .filter(|i| i.result == IncResult::Cutback)
            .count();
        let totals = vec![
            ("steps", self.steps.len().into()),
            ("increments", self.increments.len().into()),
            ("cutbacks", cutbacks.into()),
            (
                "iterations",
                self.increments.iter().map(|i| i.iters).sum::<u64>().into(),
            ),
            ("wall_s", self.wall_s.into()),
        ];
        let doc: Doc = start("fh.run", &self.header)
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

        // Failed and cutback increments first, then the others with the last one first.
        let (bad, good): (Vec<&Increment>, Vec<&Increment>) = self
            .increments
            .iter()
            .partition(|i| i.result != IncResult::Ok);
        let ranked = bad.into_iter().chain(good.into_iter().rev());
        let mut doc = tbl(
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

        if let Some(rows) = &self.contact {
            let mut rows: Vec<&ContactRow> = rows.iter().collect();
            rows.sort_by(|a, b| b.max_pen.total_cmp(&a.max_pen));
            doc = tbl(
                doc,
                "contact",
                ["pair", "closed", "max_pen", "force"],
                "no contact pairs",
                rows,
                opts.rows,
                |c| {
                    [
                        c.pair.as_str().into(),
                        c.closed.into(),
                        c.max_pen.into(),
                        c.force.into(),
                    ]
                },
            );
        }
        if let Some(f) = &self.failure {
            doc = tbl(
                doc,
                "residuals",
                ["iter", "force", "disp", "contact_changes"],
                "no residuals",
                &f.residuals,
                opts.rows,
                |r| {
                    [
                        r.iter.into(),
                        r.force.into(),
                        r.disp.into(),
                        r.contact_changes.into(),
                    ]
                },
            );
            let mut worst: Vec<&Worst> = f.worst.iter().collect();
            worst.sort_by(|a, b| b.force.total_cmp(&a.force));
            worst.truncate(MAX_WORST);
            doc = tbl(
                doc,
                "worst",
                ["label", "set", "force"],
                "no residual nodes",
                worst,
                opts.rows,
                |w| {
                    [
                        w.label.as_str().into(),
                        w.set.as_str().into(),
                        w.force.into(),
                    ]
                },
            );
        }
        finish(doc, &self.diagnostics, &self.next, opts).encode(opts)
    }
}
