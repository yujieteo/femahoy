//! `fh.validate` and `fh.validate.case` (schema sections 3.3 and 3.4).

use crate::common::{finish, start, tbl, Diag, Header, Next};
use crate::wrap::{Cell, Doc, Error, Opts};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseStatus {
    Pass,
    Fail,
    Skip,
}

impl CaseStatus {
    fn as_str(self) -> &'static str {
        match self {
            CaseStatus::Pass => "pass",
            CaseStatus::Fail => "fail",
            CaseStatus::Skip => "skip",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CaseRow {
    pub id: String,
    pub class: String,
    pub status: CaseStatus,
    /// The worst error divided by its tolerance. A value of 1 or more fails.
    pub margin: f64,
}

#[derive(Clone, Debug)]
pub struct Validate {
    pub header: Header,
    pub cases: Vec<CaseRow>,
    pub diagnostics: Vec<Diag>,
    pub next: Vec<Next>,
}

impl Validate {
    pub fn render(&self, opts: &Opts) -> Result<String, Error> {
        let count = |status: CaseStatus| self.cases.iter().filter(|c| c.status == status).count();
        let totals = vec![
            ("cases", self.cases.len().into()),
            ("pass", count(CaseStatus::Pass).into()),
            ("fail", count(CaseStatus::Fail).into()),
            ("skip", count(CaseStatus::Skip).into()),
        ];
        let mut cases: Vec<&CaseRow> = self.cases.iter().collect();
        cases.sort_by(|a, b| {
            (a.status != CaseStatus::Fail)
                .cmp(&(b.status != CaseStatus::Fail))
                .then_with(|| b.margin.total_cmp(&a.margin))
        });
        let doc: Doc = start("fh.validate", &self.header, opts).block("totals", totals);
        let doc = tbl(
            doc,
            "cases",
            ["id", "class", "status", "margin"],
            "no cases",
            cases,
            opts.rows,
            |c| {
                [
                    c.id.as_str().into(),
                    c.class.as_str().into(),
                    c.status.as_str().into(),
                    Cell::Margin(c.margin),
                ]
            },
        );
        finish(doc, &self.diagnostics, &self.next, opts).encode(opts)
    }
}

#[derive(Clone, Debug)]
pub struct Quantity {
    pub name: String,
    pub value: f64,
    pub reference: f64,
    pub margin: f64,
}

/// Class L table.
#[derive(Clone, Debug)]
pub struct Refinement {
    pub mesh: String,
    pub size: f64,
    pub value: f64,
}

/// Class R table.
#[derive(Clone, Debug)]
pub struct Rate {
    pub field: String,
    pub observed: f64,
    pub expected: f64,
}

/// Table that `--full` adds.
#[derive(Clone, Debug)]
pub struct Tolerance {
    pub name: String,
    pub tolerance: f64,
    pub source: String,
}

#[derive(Clone, Debug)]
pub struct ValidateCase {
    pub header: Header,
    pub quantities: Vec<Quantity>,
    /// `None` when the case class has no such table.
    pub refinement: Option<Vec<Refinement>>,
    pub rates: Option<Vec<Rate>>,
    pub tolerance: Option<Vec<Tolerance>>,
    pub diagnostics: Vec<Diag>,
    pub next: Vec<Next>,
}

impl ValidateCase {
    pub fn render(&self, opts: &Opts) -> Result<String, Error> {
        let fails = self.quantities.iter().filter(|q| q.margin >= 1.0).count();
        let totals = vec![
            ("quantities", self.quantities.len().into()),
            ("fail", fails.into()),
        ];
        let doc: Doc = start("fh.validate.case", &self.header, opts).block("totals", totals);
        let mut doc = tbl(
            doc,
            "quantities",
            ["name", "value", "reference", "margin"],
            "no quantities",
            &self.quantities,
            opts.rows,
            |q| {
                [
                    q.name.as_str().into(),
                    q.value.into(),
                    q.reference.into(),
                    Cell::Margin(q.margin),
                ]
            },
        );
        if let Some(rows) = &self.refinement {
            doc = tbl(
                doc,
                "refinement",
                ["mesh", "size", "value"],
                "no meshes",
                rows,
                opts.rows,
                |r| [r.mesh.as_str().into(), r.size.into(), r.value.into()],
            );
        }
        if let Some(rows) = &self.rates {
            doc = tbl(
                doc,
                "rates",
                ["field", "observed", "expected"],
                "no rates",
                rows,
                opts.rows,
                |r| {
                    [
                        r.field.as_str().into(),
                        r.observed.into(),
                        r.expected.into(),
                    ]
                },
            );
        }
        if let Some(rows) = &self.tolerance {
            doc = tbl(
                doc,
                "tolerance",
                ["name", "tolerance", "source"],
                "no tolerances",
                rows,
                opts.rows,
                |t| {
                    [
                        t.name.as_str().into(),
                        t.tolerance.into(),
                        t.source.as_str().into(),
                    ]
                },
            );
        }
        finish(doc, &self.diagnostics, &self.next, opts).encode(opts)
    }
}
