//! Build the typed model from keyword blocks.

use std::collections::HashSet;

use crate::error::code;
use crate::lexer::{lex, Block, DataLine};
use crate::model::*;
use crate::ParseError;

/// Parse `.inp` text into a model. The error is the first one that `parse_all` finds.
pub fn parse(text: &str) -> Result<Model, ParseError> {
    parse_all(text).map_err(|mut errors| errors.swap_remove(0))
}

/// Parse `.inp` text from the file `file`. Every error names the file.
pub fn parse_file(file: &str, text: &str) -> Result<Model, Vec<ParseError>> {
    parse_all(text).map_err(|errors| errors.into_iter().map(|e| e.with_file(file)).collect())
}

/// Parse `.inp` text and collect the errors of all keyword blocks, in line order. Errors
/// about a block that never ends come last.
/// A bad block adds to the list and parsing goes on with the next block. The
/// list is never empty.
pub fn parse_all(text: &str) -> Result<Model, Vec<ParseError>> {
    let mut p = Parser::default();
    let (blocks, mut errors) = lex(text);
    for block in &blocks {
        if let Err(e) = p.block(block) {
            errors.push(e);
            // A failed *END STEP still ends the step, so no second error follows.
            if block.keyword == "END STEP" {
                p.open_step = None;
            }
            p.static_failed |= block.keyword == "STATIC";
        }
    }
    errors.sort_by_key(|e| e.line);
    // Errors of an unfinished block come last: only the end of the text shows them.
    if p.open_step.is_some() {
        errors.push(ParseError::new(
            code::INCOMPLETE,
            p.step_line,
            Some("STEP"),
            "step has no *END STEP",
        ));
    }
    if errors.is_empty() {
        Ok(p.model)
    } else {
        Err(errors)
    }
}

#[derive(Default)]
struct Parser {
    model: Model,
    node_ids: HashSet<u32>,
    element_ids: HashSet<u32>,
    open_step: Option<Step>,
    step_line: usize,
    /// A `*STEP` came. Model data must come before it.
    step_seen: bool,
    /// A `*STATIC` had an error, so a missing procedure is not a second error.
    static_failed: bool,
    /// The open step has its `*STATIC`.
    step_static: bool,
    /// Index into `model.materials` of the material that is open.
    material: Option<usize>,
}

fn int(b: &Block, d: &DataLine, i: usize, what: &str) -> Result<u32, ParseError> {
    let f = d.fields.get(i).map(String::as_str).unwrap_or("");
    f.parse::<u32>().ok().filter(|&n| n > 0).ok_or_else(|| {
        b.error(
            code::INVALID_VALUE,
            d.line_of(i),
            format!("{what} must be a positive integer, found '{f}'"),
        )
    })
}

/// A finite number. The Rust parser also accepts NaN, infinity and numbers that overflow to
/// infinity, and these would change or remove a constraint without an error.
fn real(b: &Block, d: &DataLine, i: usize, what: &str) -> Result<f64, ParseError> {
    let f = d.fields.get(i).map(String::as_str).unwrap_or("");
    f.parse::<f64>().ok().filter(|x| x.is_finite()).ok_or_else(|| {
        b.error(
            code::INVALID_VALUE,
            d.line_of(i),
            format!("{what} must be a finite number, found '{f}'"),
        )
    })
}

fn opt_real(b: &Block, d: &DataLine, i: usize, what: &str) -> Result<Option<f64>, ParseError> {
    match d.fields.get(i).map(String::as_str) {
        None | Some("") => Ok(None),
        Some(_) => real(b, d, i, what).map(Some),
    }
}

fn dof(b: &Block, d: &DataLine, i: usize) -> Result<u8, ParseError> {
    let n = int(b, d, i, "degree of freedom")?;
    if (1..=6).contains(&n) {
        Ok(n as u8)
    } else {
        Err(b.error(
            code::INVALID_VALUE,
            d.line_of(i),
            format!("degree of freedom must be 1 to 6, found {n}"),
        ))
    }
}

/// The only element type: the 10-node tetrahedron.
const ELEMENT_TYPE: &str = "C3D10";
const ELEMENT_NODES: usize = 10;

impl Parser {
    fn block(&mut self, b: &Block) -> Result<(), ParseError> {
        // A material keyword (*ELASTIC, *EXPANSION) only applies inside *MATERIAL.
        if !["ELASTIC", "EXPANSION", "MATERIAL"].contains(&b.keyword.as_str()) {
            self.material = None;
        }
        match b.keyword.as_str() {
            "NODE" => self.node(b),
            "ELEMENT" => self.element(b),
            "NSET" => self.nset(b),
            "ELSET" => self.elset(b),
            "MATERIAL" => self.material(b),
            "ELASTIC" => self.elastic(b),
            "EXPANSION" => self.expansion(b),
            "INITIAL CONDITIONS" => self.initial_conditions(b),
            "TEMPERATURE" => self.temperature(b),
            "STEP" => self.step(b),
            "STATIC" => self.static_proc(b),
            "BOUNDARY" => self.boundary(b),
            "CLOAD" => self.cload(b),
            "DLOAD" => self.dload(b),
            "DSLOAD" => self.dsload(b),
            "SOLID SECTION" => self.solid_section(b),
            "SURFACE" => self.surface(b),
            "END STEP" => self.end_step(b),
            "PART" | "END PART" | "ASSEMBLY" | "END ASSEMBLY" | "INSTANCE" | "END INSTANCE" => {
                Err(b.error(
                    code::UNSUPPORTED_KEYWORD,
                    b.line,
                    "parts and assemblies are not supported; write a flat model",
                ))
            }
            _ => Err(b.error(code::UNSUPPORTED_KEYWORD, b.line, "unsupported keyword")),
        }
    }

    fn node(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NSET"])?;
        self.model_data(b)?;
        let mut ids = Vec::new();
        for d in &b.data {
            if d.fields.len() < 3 {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    d.line,
                    "node line needs an id and at least 2 coordinates",
                ));
            }
            if d.fields.len() > 4 {
                return Err(b.error(code::BAD_DATA_LINE,
                    d.line,
                    "node line has more than 3 coordinates (a line that ends in a comma joins the next line)",
                ));
            }
            let id = int(b, d, 0, "node id")?;
            let mut xyz = [0.0; 3];
            for (k, c) in xyz.iter_mut().enumerate().take(d.fields.len() - 1) {
                *c = real(b, d, k + 1, "coordinate")?;
            }
            if !self.node_ids.insert(id) {
                return Err(b.error(
                    code::DUPLICATE,
                    d.line,
                    format!("node {id} is defined twice"),
                ));
            }
            self.model.nodes.push(Node { id, xyz });
            ids.push(id);
        }
        if let Some(name) = b.value("NSET")? {
            self.add_nset(name, ids);
        }
        Ok(())
    }

    fn element(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE", "ELSET"])?;
        self.model_data(b)?;
        let ty = b.value("TYPE")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter TYPE is required",
            )
        })?;
        if ty != ELEMENT_TYPE {
            return Err(b.error(
                code::UNSUPPORTED_VALUE,
                b.line,
                format!("unsupported element type {ty}; only C3D10 is supported"),
            ));
        }
        let count = ELEMENT_NODES;
        let mut ids = Vec::new();
        for d in &b.data {
            if d.fields.len() != count + 1 {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    d.line,
                    format!(
                        "{ty} needs an id and {count} nodes, found {} fields",
                        d.fields.len()
                    ),
                ));
            }
            let id = int(b, d, 0, "element id")?;
            let mut nodes = Vec::with_capacity(count);
            for k in 1..=count {
                let n = int(b, d, k, "node id")?;
                if !self.node_ids.contains(&n) {
                    return Err(b.error(
                        code::UNDEFINED,
                        d.line_of(k),
                        format!("node {n} is not defined"),
                    ));
                }
                nodes.push(n);
            }
            if !self.element_ids.insert(id) {
                return Err(b.error(
                    code::DUPLICATE,
                    d.line,
                    format!("element {id} is defined twice"),
                ));
            }
            self.model.elements.push(Element {
                id,
                element_type: ELEMENT_TYPE.to_owned(),
                nodes,
            });
            ids.push(id);
        }
        if let Some(name) = b.value("ELSET")? {
            self.add_elset(name, ids);
        }
        Ok(())
    }

    fn nset(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NSET", "GENERATE"])?;
        self.model_data(b)?;
        let name = b.value("NSET")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter NSET is required",
            )
        })?;
        let ids = self.set_members(b, true)?;
        self.add_nset(name, ids);
        Ok(())
    }

    fn elset(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["ELSET", "GENERATE"])?;
        self.model_data(b)?;
        let name = b.value("ELSET")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter ELSET is required",
            )
        })?;
        let ids = self.set_members(b, false)?;
        self.add_elset(name, ids);
        Ok(())
    }

    /// Members of a set from the data lines: ids, names of earlier sets, or
    /// `start, end, step` triples with `GENERATE`.
    fn set_members(&self, b: &Block, nodes: bool) -> Result<Vec<u32>, ParseError> {
        let (kind, known) = if nodes {
            ("node", &self.node_ids)
        } else {
            ("element", &self.element_ids)
        };
        let generate = b.param("GENERATE").is_some();
        let mut out = Vec::new();
        for d in &b.data {
            if generate {
                let start = int(b, d, 0, "first id")?;
                let end = int(b, d, 1, "last id")?;
                let step = if d.fields.len() > 2 {
                    int(b, d, 2, "increment")?
                } else {
                    1
                };
                if step == 0 || end < start {
                    return Err(b.error(
                        code::INVALID_VALUE,
                        d.line,
                        "GENERATE needs first <= last and increment > 0",
                    ));
                }
                for id in (start..=end).step_by(step as usize) {
                    if !known.contains(&id) {
                        return Err(b.error(
                            code::UNDEFINED,
                            d.line,
                            format!("{kind} {id} is not defined"),
                        ));
                    }
                    out.push(id);
                }
                continue;
            }
            for f in d.fields.iter().filter(|f| !f.is_empty()) {
                if let Ok(id) = f.parse::<u32>() {
                    if !known.contains(&id) {
                        return Err(b.error(
                            code::UNDEFINED,
                            d.line,
                            format!("{kind} {id} is not defined"),
                        ));
                    }
                    out.push(id);
                    continue;
                }
                let upper = f.to_uppercase();
                let members = if nodes {
                    self.model
                        .nsets
                        .iter()
                        .find(|s| s.name == upper)
                        .map(|s| &s.ids)
                } else {
                    self.model
                        .elsets
                        .iter()
                        .find(|s| s.name == upper)
                        .map(|s| &s.ids)
                };
                match members {
                    Some(ids) => out.extend(ids),
                    None => {
                        return Err(b.error(
                            code::UNDEFINED,
                            d.line,
                            format!("{kind} set {upper} is not defined"),
                        ))
                    }
                }
            }
        }
        Ok(out)
    }

    fn add_nset(&mut self, name: &str, ids: Vec<u32>) {
        match self.model.nsets.iter_mut().find(|s| s.name == name) {
            Some(s) => s.ids.extend(ids),
            None => self.model.nsets.push(NodeSet {
                name: name.to_owned(),
                ids,
            }),
        }
    }

    fn add_elset(&mut self, name: &str, ids: Vec<u32>) {
        match self.model.elsets.iter_mut().find(|s| s.name == name) {
            Some(s) => s.ids.extend(ids),
            None => self.model.elsets.push(ElementSet {
                name: name.to_owned(),
                ids,
            }),
        }
    }

    fn material(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NAME"])?;
        self.model_data(b)?;
        let name = b.value("NAME")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter NAME is required",
            )
        })?;
        if let Some(d) = b.data.first() {
            return Err(b.error(code::BAD_DATA_LINE, d.line, "keyword takes no data lines"));
        }
        if self.model.material(name).is_some() {
            return Err(b.error(
                code::DUPLICATE,
                b.line,
                format!("material {name} is defined twice"),
            ));
        }
        self.model.materials.push(Material {
            name: name.to_owned(),
            elastic: None,
            expansion: None,
        });
        self.material = Some(self.model.materials.len() - 1);
        Ok(())
    }

    fn elastic(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE"])?;
        if let Some(t) = b.value("TYPE")? {
            if t != "ISOTROPIC" {
                return Err(b.error(
                    code::UNSUPPORTED_VALUE,
                    b.line,
                    format!("unsupported TYPE {t}"),
                ));
            }
        }
        let idx = self.material.ok_or_else(|| {
            b.error(
                code::OUT_OF_PLACE,
                b.line,
                "keyword needs an open *MATERIAL",
            )
        })?;
        let [d] = b.data.as_slice() else {
            return Err(b.error(
                code::BAD_DATA_LINE,
                b.line,
                "keyword needs exactly 1 data line",
            ));
        };
        let elastic = Elastic {
            youngs_modulus: real(b, d, 0, "Young's modulus")?,
            poissons_ratio: real(b, d, 1, "Poisson's ratio")?,
        };
        if elastic.youngs_modulus <= 0.0 {
            return Err(b.error(
                code::INVALID_VALUE,
                d.line,
                "Young's modulus must be positive",
            ));
        }
        // At -1 the shear modulus has no value: it divides by 1 + nu.
        if !(elastic.poissons_ratio > -1.0 && elastic.poissons_ratio < 0.5) {
            return Err(b.error(
                code::INVALID_VALUE,
                d.line,
                "Poisson's ratio must be in (-1, 0.5)",
            ));
        }
        let mat = &mut self.model.materials[idx];
        if mat.elastic.is_some() {
            return Err(b.error(code::DUPLICATE, b.line, "material already has *ELASTIC"));
        }
        mat.elastic = Some(elastic);
        Ok(())
    }

    /// `*EXPANSION`: one isotropic coefficient alpha. The thermal strain is alpha (T - T0).
    /// T0 is the initial temperature of the node from `*INITIAL CONDITIONS`.
    fn expansion(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE"])?;
        if let Some(t) = b.value("TYPE")? {
            if t != "ISO" {
                return Err(b.error(
                    code::UNSUPPORTED_VALUE,
                    b.line,
                    format!("unsupported TYPE {t}"),
                ));
            }
        }
        let idx = self.material.ok_or_else(|| {
            b.error(
                code::OUT_OF_PLACE,
                b.line,
                "keyword needs an open *MATERIAL",
            )
        })?;
        let d = match b.data.as_slice() {
            [d] if d.fields.len() == 1 => d,
            _ => {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    b.data.first().map_or(b.line, |d| d.line),
                    "keyword takes 1 data line with 1 value",
                ))
            }
        };
        let alpha = real(b, d, 0, "expansion coefficient")?;
        let mat = &mut self.model.materials[idx];
        if mat.expansion.is_some() {
            return Err(b.error(code::DUPLICATE, b.line, "material already has *EXPANSION"));
        }
        mat.expansion = Some(alpha);
        Ok(())
    }

    /// Data lines of a node or node set and a temperature.
    fn temperatures(&self, b: &Block) -> Result<Vec<Temperature>, ParseError> {
        b.data
            .iter()
            .map(|d| {
                if d.fields.len() != 2 {
                    return Err(b.error(
                        code::BAD_DATA_LINE,
                        d.line,
                        "line needs node and temperature",
                    ));
                }
                Ok(Temperature {
                    target: self.target(b, d)?,
                    value: real(b, d, 1, "temperature")?,
                })
            })
            .collect()
    }

    fn initial_conditions(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE"])?;
        match b.value("TYPE")? {
            Some("TEMPERATURE") => {}
            Some(t) => {
                return Err(b.error(
                    code::UNSUPPORTED_VALUE,
                    b.line,
                    format!("unsupported TYPE {t}"),
                ))
            }
            None => {
                return Err(b.error(
                    code::MISSING_PARAMETER,
                    b.line,
                    "parameter TYPE is required",
                ))
            }
        }
        self.model_data(b)?;
        let list = self.temperatures(b)?;
        self.model.initial_temperatures.extend(list);
        Ok(())
    }

    fn temperature(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "keyword needs an open *STEP"));
        }
        let list = self.temperatures(b)?;
        if let Some(step) = self.open_step.as_mut() {
            step.temperatures.extend(list);
        }
        Ok(())
    }

    fn step(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NAME"])?;
        if self.open_step.is_some() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "step inside a step"));
        }
        if let Some(d) = b.data.first() {
            return Err(b.error(code::BAD_DATA_LINE, d.line, "keyword takes no data lines"));
        }
        self.step_line = b.line;
        self.step_seen = true;
        self.step_static = false;
        self.open_step = Some(Step {
            name: b.value("NAME")?.map(str::to_owned),
            ..Step::default()
        });
        Ok(())
    }

    /// `*STATIC`: the adapter writes its own `*STATIC` line, so the data lines are not read.
    fn static_proc(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "keyword needs an open *STEP"));
        }
        if std::mem::replace(&mut self.step_static, true) {
            return Err(b.error(code::DUPLICATE, b.line, "step already has *STATIC"));
        }
        Ok(())
    }

    fn target(&self, b: &Block, d: &DataLine) -> Result<Target, ParseError> {
        let f = d.fields.first().map(String::as_str).unwrap_or("");
        if let Ok(id) = f.parse::<u32>() {
            if !self.node_ids.contains(&id) {
                return Err(b.error(code::UNDEFINED, d.line, format!("node {id} is not defined")));
            }
            return Ok(Target::Node(id));
        }
        let name = f.to_uppercase();
        if name.is_empty() || self.model.nset(&name).is_none() {
            return Err(b.error(
                code::UNDEFINED,
                d.line,
                format!("node set '{name}' is not defined"),
            ));
        }
        Ok(Target::Set(name))
    }

    fn boundary(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() < 2 || d.fields.len() > 4 {
                return Err(b.error(
                    code::INVALID_VALUE,
                    d.line,
                    "line needs node, first dof, and optional last dof and value",
                ));
            }
            let first_dof = dof(b, d, 1)?;
            let last_dof = if d.fields.get(2).is_some_and(|f| !f.is_empty()) {
                dof(b, d, 2)?
            } else {
                first_dof
            };
            if last_dof < first_dof {
                return Err(b.error(code::INVALID_VALUE, d.line, "last dof is below first dof"));
            }
            let value = opt_real(b, d, 3, "value")?.unwrap_or(0.0);
            out.push(Boundary {
                target: self.target(b, d)?,
                first_dof,
                last_dof,
                value,
            });
        }
        match self.open_step.as_mut() {
            Some(step) => step.boundaries.extend(out),
            None => self.model.boundaries.extend(out),
        }
        Ok(())
    }

    fn cload(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "keyword needs an open *STEP"));
        }
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() != 3 {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    d.line,
                    "line needs node, dof and magnitude",
                ));
            }
            out.push(Cload {
                target: self.target(b, d)?,
                dof: dof(b, d, 1)?,
                magnitude: real(b, d, 2, "magnitude")?,
            });
        }
        if let Some(step) = self.open_step.as_mut() {
            step.cloads.extend(out);
        }
        Ok(())
    }

    /// Model data applies to all steps, so it must come before the first step starts.
    fn model_data(&self, b: &Block) -> Result<(), ParseError> {
        if self.step_seen {
            return Err(b.error(
                code::OUT_OF_PLACE,
                b.line,
                "keyword must come before the first *STEP",
            ));
        }
        Ok(())
    }

    fn solid_section(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["ELSET", "MATERIAL"])?;
        self.model_data(b)?;
        let elset = b.value("ELSET")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter ELSET is required",
            )
        })?;
        let material = b.value("MATERIAL")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter MATERIAL is required",
            )
        })?;
        if let Some(d) = b.data.first() {
            return Err(b.error(code::BAD_DATA_LINE, d.line, "keyword takes no data lines"));
        }
        if self.model.elset(elset).is_none() {
            return Err(b.error(
                code::UNDEFINED,
                b.line,
                format!("element set '{elset}' is not defined"),
            ));
        }
        if self.model.material(material).is_none() {
            return Err(b.error(
                code::UNDEFINED,
                b.line,
                format!("material '{material}' is not defined"),
            ));
        }
        self.model.sections.push(SolidSection {
            elset: elset.to_owned(),
            material: material.to_owned(),
        });
        Ok(())
    }

    fn surface(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE", "NAME"])?;
        self.model_data(b)?;
        match b.value("TYPE")? {
            Some("ELEMENT") => {}
            Some(t) => {
                return Err(b.error(
                    code::UNSUPPORTED_VALUE,
                    b.line,
                    format!("unsupported TYPE {t}"),
                ))
            }
            None => {
                return Err(b.error(
                    code::MISSING_PARAMETER,
                    b.line,
                    "parameter TYPE is required",
                ))
            }
        }
        let name = b.value("NAME")?.ok_or_else(|| {
            b.error(
                code::MISSING_PARAMETER,
                b.line,
                "parameter NAME is required",
            )
        })?;
        if self.model.element_surface(name).is_some() {
            return Err(b.error(
                code::DUPLICATE,
                b.line,
                format!("surface '{name}' is already defined"),
            ));
        }
        self.element_surface(b, name)
    }

    /// Data lines: `element or element set, S<face>`.
    fn element_surface(&mut self, b: &Block, name: &str) -> Result<(), ParseError> {
        let mut faces = Vec::new();
        for d in &b.data {
            if d.fields.len() != 2 {
                return Err(b.error(code::BAD_DATA_LINE, d.line, "line needs element and face"));
            }
            let ty = d.fields[1].to_uppercase();
            let face = ty
                .strip_prefix('S')
                .and_then(|n| n.parse::<u8>().ok())
                .ok_or_else(|| {
                    b.error(
                        code::UNSUPPORTED_VALUE,
                        d.line,
                        format!("unsupported face {ty}"),
                    )
                })?;
            let f = d.fields[0].as_str();
            let ids = if let Ok(id) = f.parse::<u32>() {
                if !self.element_ids.contains(&id) {
                    return Err(b.error(
                        code::UNDEFINED,
                        d.line,
                        format!("element {id} is not defined"),
                    ));
                }
                vec![id]
            } else {
                let set = f.to_uppercase();
                match self.model.elset(&set) {
                    Some(s) => s.ids.clone(),
                    None => {
                        return Err(b.error(
                            code::UNDEFINED,
                            d.line,
                            format!("element set '{set}' is not defined"),
                        ))
                    }
                }
            };
            if !(1..=4).contains(&face) {
                return Err(b.error(
                    code::INVALID_VALUE,
                    d.line,
                    format!("face must be 1 to 4, found {face}"),
                ));
            }
            faces.extend(ids.into_iter().map(|id| (id, face)));
        }
        self.model.element_surfaces.push(ElementSurface {
            name: name.to_owned(),
            faces,
        });
        Ok(())
    }

    fn dload(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "keyword needs an open *STEP"));
        }
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() != 3 {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    d.line,
                    "line needs element, load type and magnitude",
                ));
            }
            let ty = d.fields[1].as_str();
            let face = ty
                .strip_prefix(['P', 'p'])
                .and_then(|n| n.parse::<u8>().ok())
                .ok_or_else(|| {
                    b.error(
                        code::UNSUPPORTED_VALUE,
                        d.line,
                        format!("unsupported load type {ty}"),
                    )
                })?;
            if !(1..=4).contains(&face) {
                return Err(b.error(
                    code::INVALID_VALUE,
                    d.line,
                    format!("face must be 1 to 4, found {face}"),
                ));
            }
            let f = d.fields[0].as_str();
            let target = if let Ok(id) = f.parse::<u32>() {
                if !self.element_ids.contains(&id) {
                    return Err(b.error(
                        code::UNDEFINED,
                        d.line,
                        format!("element {id} is not defined"),
                    ));
                }
                ElementTarget::Element(id)
            } else {
                let name = f.to_uppercase();
                if self.model.elset(&name).is_none() {
                    return Err(b.error(
                        code::UNDEFINED,
                        d.line,
                        format!("element set '{name}' is not defined"),
                    ));
                }
                ElementTarget::Set(name)
            };
            out.push(Dload {
                target,
                face,
                pressure: real(b, d, 2, "magnitude")?,
            });
        }
        if let Some(step) = self.open_step.as_mut() {
            step.dloads.extend(out);
        }
        Ok(())
    }

    fn dsload(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(code::OUT_OF_PLACE, b.line, "keyword needs an open *STEP"));
        }
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() != 3 {
                return Err(b.error(
                    code::BAD_DATA_LINE,
                    d.line,
                    "line needs surface, load type and magnitude",
                ));
            }
            let name = d.fields[0].as_str();
            let Some(surface) = self.model.element_surface(name) else {
                return Err(b.error(
                    code::UNDEFINED,
                    d.line,
                    format!("element surface '{name}' is not defined"),
                ));
            };
            let name = surface.name.clone();
            let ty = d.fields[1].as_str();
            if !ty.eq_ignore_ascii_case("P") {
                return Err(b.error(
                    code::UNSUPPORTED_VALUE,
                    d.line,
                    format!("unsupported load type {ty}"),
                ));
            }
            out.push(Dsload {
                surface: name,
                pressure: real(b, d, 2, "magnitude")?,
            });
        }
        if let Some(step) = self.open_step.as_mut() {
            step.dsloads.extend(out);
        }
        Ok(())
    }

    fn end_step(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if let Some(d) = b.data.first() {
            return Err(b.error(code::BAD_DATA_LINE, d.line, "keyword takes no data lines"));
        }
        let step = self
            .open_step
            .take()
            .ok_or_else(|| b.error(code::OUT_OF_PLACE, b.line, "no open *STEP"))?;
        if !self.step_static && !std::mem::take(&mut self.static_failed) {
            return Err(b.error(
                code::INCOMPLETE,
                self.step_line,
                "step has no procedure (*STATIC)",
            ));
        }
        self.model.steps.push(step);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nodes of 1 C3D10: 4 corners, then 6 midside nodes. The mesh ends at line 11.
    const MESH: &str = "*NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,.5,0,0\n6,.5,.5,0\n\
                        7,0,.5,0\n8,0,0,.5\n9,.5,0,.5\n10,0,.5,.5\n";

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn unsupported_keyword_names_line_and_keyword() {
        assert_eq!(
            err("*NODE\n1,0,0,0\n*PLASTIC\n"),
            "line 3, *PLASTIC: unsupported keyword (INP002)"
        );
    }

    #[test]
    fn contact_and_constraint_keywords_are_rejected() {
        for k in [
            "CONTACT PAIR",
            "SURFACE INTERACTION, NAME=A",
            "COUPLING, CONSTRAINT NAME=C, REF NODE=1, SURFACE=S",
            "KINEMATIC",
            "DISTRIBUTING",
            "EQUATION",
            "MPC",
        ] {
            assert!(
                err(&format!("*{k}\n")).contains("unsupported keyword"),
                "{k}"
            );
        }
    }

    #[test]
    fn an_error_on_a_continuation_line_names_that_line() {
        let mut bad = Vec::new();
        for (text, want) in [
            // Line 14 is the second physical line of the record.
            (
                "*ELEMENT, TYPE=C3D10\n1,1,2,3,4,5,\n6,7,8,9,99\n",
                "line 14, *ELEMENT: node 99",
            ),
            (
                "*ELEMENT, TYPE=C3D10\n1,1,2,3,4,5,\n6,7,8,9,x\n",
                "line 14, *ELEMENT: node id",
            ),
            ("*NODE\n11,0,\n0,x\n", "line 14, *NODE: "),
        ] {
            let e = err(&format!("{MESH}{text}"));
            if !e.starts_with(want) {
                bad.push(format!("{text:?}: {e}"));
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn a_data_line_after_end_step_is_an_error() {
        assert_eq!(
            err("*STEP\n*STATIC\n*END STEP\n99,unsupported\n"),
            "line 4, *END STEP: keyword takes no data lines (INP007)"
        );
    }

    #[test]
    fn a_zero_label_is_an_error() {
        assert_eq!(
            err("*NODE\n0,0,0,0\n"),
            "line 2, *NODE: node id must be a positive integer, found '0' (INP006)"
        );
        let element = format!("{MESH}*ELEMENT, TYPE=C3D10\n0,1,2,3,4,5,6,7,8,9,10\n");
        assert_eq!(
            err(&element),
            "line 13, *ELEMENT: element id must be a positive integer, found '0' (INP006)"
        );
    }

    #[test]
    fn element_errors_name_the_line() {
        let prefix = format!("{MESH}*ELEMENT, TYPE=c3d10\n");
        let model = parse(&format!("{prefix}1,1,2,3,4,5,6,7,8,9,10\n")).unwrap();
        assert_eq!(model.elements[0].element_type, "C3D10");
        assert_eq!(model.elements[0].nodes, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        for nodes in ["1,2,3", "1,2,3,4,5,6,7,8,9,10,1"] {
            assert!(err(&format!("{prefix}1,{nodes}\n"))
                .starts_with("line 13, *ELEMENT: C3D10 needs an id and 10 nodes"));
        }
        assert_eq!(
            err(&format!("{prefix}1,1,2,3,4,5,6,7,8,9,99\n")),
            "line 13, *ELEMENT: node 99 is not defined (INP009)"
        );
        for ty in ["S4R", "C3D4", "C3D8", "C3D8I", "C3D10M"] {
            assert_eq!(
                err(&format!("{MESH}*ELEMENT, TYPE={ty}\n")),
                format!(
                    "line 12, *ELEMENT: unsupported element type {ty}; only C3D10 is supported \
                     (INP005)"
                )
            );
        }
    }

    #[test]
    fn bad_number_names_line() {
        assert_eq!(
            err("*NODE\n1,0,x,0\n"),
            "line 2, *NODE: coordinate must be a finite number, found 'x' (INP006)"
        );
    }

    #[test]
    fn duplicate_node_fails() {
        assert_eq!(
            err("*NODE\n1,0,0,0\n1,1,0,0\n"),
            "line 3, *NODE: node 1 is defined twice (INP008)"
        );
    }

    #[test]
    fn a_node_line_with_more_than_3_coordinates_fails() {
        // A trailing comma joins the next line, so node 2 would go into node 1.
        for text in ["*NODE\n1,0,0,\n2,1,0\n", "*NODE\n1,0,0,0,5\n"] {
            let e = err(text);
            assert!(
                e.starts_with("line 2, *NODE: node line has more than 3 coordinates"),
                "{e}"
            );
        }
    }

    #[test]
    fn unknown_parameter_fails() {
        assert_eq!(
            err("*NODE, FOO=1\n"),
            "line 1, *NODE: unsupported parameter FOO (INP003)"
        );
    }

    #[test]
    fn elastic_outside_material_fails() {
        assert_eq!(
            err("*ELASTIC\n1, 0.3\n"),
            "line 1, *ELASTIC: keyword needs an open *MATERIAL (INP010)"
        );
        assert!(err("*MATERIAL, NAME=A\n*NODE\n*ELASTIC\n1,0.3\n").contains("line 3, *ELASTIC"));
    }

    #[test]
    fn step_rules() {
        assert_eq!(
            err("*STEP\n"),
            "line 1, *STEP: step has no *END STEP (INP011)"
        );
        assert_eq!(
            err("*STEP\n*END STEP\n"),
            "line 1, *END STEP: step has no procedure (*STATIC) (INP011)"
        );
        assert_eq!(
            err("*STATIC\n"),
            "line 1, *STATIC: keyword needs an open *STEP (INP010)"
        );
        assert_eq!(
            err("*STEP\n*STEP\n"),
            "line 2, *STEP: step inside a step (INP010)"
        );
        assert_eq!(
            err("*CLOAD\n"),
            "line 1, *CLOAD: keyword needs an open *STEP (INP010)"
        );
        assert_eq!(
            err("*STEP, NLGEOM=YES\n*STATIC\n*END STEP\n"),
            "line 1, *STEP: unsupported parameter NLGEOM (INP003)"
        );
        assert_eq!(
            err("*STEP\n*STATIC\n*STATIC\n*END STEP\n"),
            "line 3, *STATIC: step already has *STATIC (INP008)"
        );
    }

    #[test]
    fn a_node_surface_is_rejected() {
        assert_eq!(
            err(&format!("{MESH}*SURFACE, TYPE=NODE, NAME=S\n1\n")),
            "line 12, *SURFACE: unsupported TYPE NODE (INP005)"
        );
    }

    #[test]
    fn boundary_rules() {
        let head = format!("{MESH}*NSET, NSET=A\n1, 2\n");
        assert!(err(&format!("{head}*BOUNDARY\nB, 1\n")).contains("node set 'B' is not defined"));
        assert!(
            err(&format!("{head}*BOUNDARY\n1, 7\n")).contains("degree of freedom must be 1 to 6")
        );
        assert!(err(&format!("{head}*BOUNDARY\n1, 3, 1\n")).contains("last dof is below first dof"));
        let m = parse(&format!("{head}*BOUNDARY\n1, 1, 2, 0.5\na, 3\n")).unwrap();
        assert_eq!(m.boundaries.len(), 2);
        assert_eq!(m.boundaries[0].value, 0.5);
        assert_eq!(m.boundaries[1].target, Target::Set("A".into()));
    }

    #[test]
    fn sets_merge_reference_and_generate() {
        let m = parse(&format!(
            "{MESH}*nset, nset=a\n1, 2\n*NSET, NSET=A\n3\n*NSET, NSET=B, GENERATE\n5, 8, 3\n*NSET, NSET=C\na, b\n"
        ))
        .unwrap();
        assert_eq!(m.nset("A").unwrap().ids, [1, 2, 3]);
        assert_eq!(m.nset("B").unwrap().ids, [5, 8]);
        assert_eq!(m.nset("C").unwrap().ids, [1, 2, 3, 5, 8]);
    }

    #[test]
    fn node_and_element_set_params_fill_sets() {
        let m = parse("*NODE, NSET=ALL\n1,0,0,0\n2,1,0,0\n").unwrap();
        assert_eq!(m.nset("all").unwrap().ids, [1, 2]);
    }

    #[test]
    fn full_step_parses() {
        let m = parse(&format!(
            "{MESH}*MATERIAL, NAME=M\n*ELASTIC, TYPE=ISOTROPIC\n100, 0.3\n*STEP, NAME=S\n*STATIC\n0.1, 1\n*CLOAD\n8, 3, -2.5\n*END STEP\n"
        ))
        .unwrap();
        let s = &m.steps[0];
        assert_eq!(s.name.as_deref(), Some("S"));
        assert_eq!(
            s.cloads[0],
            Cload {
                target: Target::Node(8),
                dof: 3,
                magnitude: -2.5
            }
        );
    }

    #[test]
    fn thermal_keywords_parse() {
        let m = parse(&format!(
            "{MESH}*NSET, NSET=ALL\n1, 2, 3, 4, 5, 6, 7, 8\n\
             *MATERIAL, NAME=A\n*ELASTIC\n100, 0.3\n*EXPANSION, TYPE=ISO\n1.2e-5\n\
             *MATERIAL, NAME=B\n*EXPANSION\n-2e-6\n*ELASTIC\n50, 0\n\
             *INITIAL CONDITIONS, TYPE=TEMPERATURE\nALL, 20\n\
             *STEP\n*STATIC\n*TEMPERATURE\nALL, 120\n8, 100\n*END STEP\n"
        ))
        .unwrap();
        let expansion: Vec<_> = m.materials.iter().map(|m| m.expansion).collect();
        assert_eq!(expansion, [Some(1.2e-5), Some(-2e-6)]);
        assert_eq!(
            m.materials[1].elastic.as_ref().unwrap().youngs_modulus,
            50.0
        );
        let all = Target::Set("ALL".into());
        let t = |target: &Target, value| Temperature {
            target: target.clone(),
            value,
        };
        assert_eq!(m.initial_temperatures, [t(&all, 20.0)]);
        assert_eq!(
            m.steps[0].temperatures,
            [t(&all, 120.0), t(&Target::Node(8), 100.0)]
        );
    }

    #[test]
    fn thermal_keyword_rules() {
        let mat = "*MATERIAL, NAME=M\n*ELASTIC\n100, 0.3\n";
        for (text, message) in [
            (
                "*EXPANSION\n1e-5\n".to_owned(),
                "line 1, *EXPANSION: keyword needs an open *MATERIAL (INP010)",
            ),
            (
                format!("{mat}*EXPANSION, TYPE=ORTHO\n1e-5\n"),
                "line 4, *EXPANSION: unsupported TYPE ORTHO (INP005)",
            ),
            (
                format!("{mat}*EXPANSION\n1e-5, 20\n"),
                "line 5, *EXPANSION: keyword takes 1 data line with 1 value (INP007)",
            ),
            (
                format!("{mat}*EXPANSION\n1e-5\n*EXPANSION\n1e-5\n"),
                "line 6, *EXPANSION: material already has *EXPANSION (INP008)",
            ),
            (
                format!("{MESH}*INITIAL CONDITIONS\n1, 20\n"),
                "line 12, *INITIAL CONDITIONS: parameter TYPE is required (INP004)",
            ),
            (
                format!("{MESH}*INITIAL CONDITIONS, TYPE=STRESS\n1, 20\n"),
                "line 12, *INITIAL CONDITIONS: unsupported TYPE STRESS (INP005)",
            ),
            (
                format!("{MESH}*INITIAL CONDITIONS, TYPE=TEMPERATURE\n99, 20\n"),
                "line 13, *INITIAL CONDITIONS: node 99 is not defined (INP009)",
            ),
            (
                format!("{MESH}*INITIAL CONDITIONS, TYPE=TEMPERATURE\n1, 20, 30\n"),
                "line 13, *INITIAL CONDITIONS: line needs node and temperature (INP007)",
            ),
            (
                format!("{MESH}*STEP\n*STATIC\n*INITIAL CONDITIONS, TYPE=TEMPERATURE\n1, 20\n"),
                "line 14, *INITIAL CONDITIONS: keyword must come before the first *STEP (INP010)",
            ),
            (
                format!("{MESH}*TEMPERATURE\n1, 20\n"),
                "line 12, *TEMPERATURE: keyword needs an open *STEP (INP010)",
            ),
            (
                format!("{MESH}*STEP\n*STATIC\n*TEMPERATURE, OP=NEW\n1, 20\n*END STEP\n"),
                "line 14, *TEMPERATURE: unsupported parameter OP (INP003)",
            ),
            (
                format!("{MESH}*STEP\n*STATIC\n*TEMPERATURE\n1, x\n*END STEP\n"),
                "line 15, *TEMPERATURE: temperature must be a finite number, found 'x' (INP006)",
            ),
        ] {
            assert_eq!(err(&text), message, "{text}");
        }
    }

    #[test]
    fn elastic_range_checks() {
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n100, 0.5\n").contains("Poisson's ratio"));
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n100, -1\n").contains("Poisson's ratio"));
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n-1, 0.3\n").contains("Young's modulus"));
    }
}
