//! `fh.check` (schema section 3.2).

use crate::common::{count, finish, start, tbl, Diag, Header, Next, Severity};
use crate::wrap::{Doc, Error, Opts};

#[derive(Clone, Debug)]
pub struct ElementType {
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug)]
pub struct SetRow {
    pub name: String,
    pub kind: String,
    pub size: u64,
    /// A step uses the set. Ranking only: it is not printed.
    pub used_by_step: bool,
}

#[derive(Clone, Debug)]
pub struct MaterialRow {
    pub name: String,
    pub behaviour: String,
    pub used_by: u64,
}

#[derive(Clone, Debug)]
pub struct StepRow {
    pub name: String,
    pub procedure: String,
    pub nlgeom: bool,
    pub loads: u64,
}

#[derive(Clone, Debug)]
pub struct QualityRow {
    pub metric: String,
    pub worst: f64,
    pub limit: f64,
    pub bad: u64,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub header: Header,
    pub nodes: u64,
    pub element_types: Vec<ElementType>,
    pub sets: Vec<SetRow>,
    pub materials: Vec<MaterialRow>,
    pub steps: Vec<StepRow>,
    pub quality: Vec<QualityRow>,
    pub diagnostics: Vec<Diag>,
    pub next: Vec<Next>,
}

impl Check {
    pub fn render(&self, opts: &Opts) -> Result<String, Error> {
        let elements = self
            .element_types
            .iter()
            .try_fold(0u64, |total, e| total.checked_add(e.count))
            .ok_or_else(|| Error("element count overflow".into()))?;
        let totals = vec![
            ("nodes", self.nodes.into()),
            ("elements", elements.into()),
            ("sets", self.sets.len().into()),
            ("materials", self.materials.len().into()),
            ("steps", self.steps.len().into()),
            ("errors", count(&self.diagnostics, Severity::Error).into()),
            (
                "warnings",
                count(&self.diagnostics, Severity::Warning).into(),
            ),
        ];
        let doc: Doc = start("fh.check", &self.header).block("totals", totals);

        let mut types: Vec<&ElementType> = self.element_types.iter().collect();
        types.sort_by_key(|e| std::cmp::Reverse(e.count));
        let doc = tbl(
            doc,
            "element_types",
            ["type", "count"],
            "no elements",
            types,
            opts.rows,
            |e| [e.name.as_str().into(), e.count.into()],
        );

        let mut sets: Vec<&SetRow> = self.sets.iter().collect();
        sets.sort_by(|a, b| (!a.used_by_step, &a.name).cmp(&(!b.used_by_step, &b.name)));
        let doc = tbl(
            doc,
            "sets",
            ["name", "kind", "size"],
            "no sets",
            sets,
            opts.rows,
            |s| {
                [
                    s.name.as_str().into(),
                    s.kind.as_str().into(),
                    s.size.into(),
                ]
            },
        );

        let mut materials: Vec<&MaterialRow> = self.materials.iter().collect();
        materials.sort_by(|a, b| a.name.cmp(&b.name));
        let doc = tbl(
            doc,
            "materials",
            ["name", "behaviour", "used_by"],
            "no materials",
            materials,
            opts.rows,
            |m| {
                [
                    m.name.as_str().into(),
                    m.behaviour.as_str().into(),
                    m.used_by.into(),
                ]
            },
        );

        let doc = tbl(
            doc,
            "steps",
            ["name", "procedure", "nlgeom", "loads"],
            "no steps",
            &self.steps,
            opts.rows,
            |s| {
                [
                    s.name.as_str().into(),
                    s.procedure.as_str().into(),
                    if s.nlgeom { "yes" } else { "no" }.into(),
                    s.loads.into(),
                ]
            },
        );
        let doc = tbl(
            doc,
            "quality",
            ["metric", "worst", "limit", "bad"],
            "no quality data",
            &self.quality,
            opts.rows,
            |q| {
                [
                    q.metric.as_str().into(),
                    q.worst.into(),
                    q.limit.into(),
                    q.bad.into(),
                ]
            },
        );
        finish(doc, &self.diagnostics, &self.next, opts).encode(opts)
    }
}
