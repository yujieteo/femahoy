//! Build the typed model from keyword blocks.

use std::collections::HashSet;

use crate::lexer::{lex, Block, DataLine};
use crate::model::*;
use crate::ParseError;

/// Parse `.inp` text into a model.
pub fn parse(text: &str) -> Result<Model, ParseError> {
    let mut p = Parser::default();
    for block in lex(text)? {
        p.block(&block)?;
    }
    if p.open_step.is_some() {
        return Err(ParseError::new(
            p.step_line,
            Some("STEP"),
            "step has no *END STEP",
        ));
    }
    p.close_coupling()?;
    Ok(p.model)
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
    /// Index into `model.materials` of the material that is open.
    material: Option<usize>,
    /// A `*COUPLING` that still waits for `*KINEMATIC` or `*DISTRIBUTING`.
    open_coupling: Option<PendingCoupling>,
}

struct PendingCoupling {
    line: usize,
    name: String,
    ref_node: u32,
    surface: String,
}

fn int(b: &Block, d: &DataLine, i: usize, what: &str) -> Result<u32, ParseError> {
    let f = d.fields.get(i).map(String::as_str).unwrap_or("");
    f.parse::<u32>().ok().filter(|&n| n > 0).ok_or_else(|| {
        b.error(
            d.line_of(i),
            format!("{what} must be a positive integer, found '{f}'"),
        )
    })
}

/// A finite number. The Rust parser also accepts NaN, infinity and numbers that overflow to
/// infinity, and these would change or remove a constraint without an error.
fn finite(f: &str) -> Option<f64> {
    f.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn real(b: &Block, d: &DataLine, i: usize, what: &str) -> Result<f64, ParseError> {
    let f = d.fields.get(i).map(String::as_str).unwrap_or("");
    finite(f).ok_or_else(|| {
        b.error(
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
            d.line_of(i),
            format!("degree of freedom must be 1 to 6, found {n}"),
        ))
    }
}

fn element_nodes(element_type: &str) -> Option<usize> {
    match element_type {
        "C3D8" | "C3D8I" => Some(8),
        "C3D10" | "C3D10M" => Some(10),
        _ => None,
    }
}

impl Parser {
    fn block(&mut self, b: &Block) -> Result<(), ParseError> {
        // A material keyword (*ELASTIC) only applies inside *MATERIAL.
        if b.keyword != "ELASTIC" && b.keyword != "MATERIAL" {
            self.material = None;
        }
        if b.keyword != "KINEMATIC" && b.keyword != "DISTRIBUTING" {
            self.close_coupling()?;
        }
        match b.keyword.as_str() {
            "NODE" => self.node(b),
            "ELEMENT" => self.element(b),
            "NSET" => self.nset(b),
            "ELSET" => self.elset(b),
            "MATERIAL" => self.material(b),
            "ELASTIC" => self.elastic(b),
            "STEP" => self.step(b),
            "STATIC" => self.static_proc(b),
            "BOUNDARY" => self.boundary(b),
            "CLOAD" => self.cload(b),
            "DLOAD" => self.dload(b),
            "SOLID SECTION" => self.solid_section(b),
            "SURFACE" => self.surface(b),
            "COUPLING" => self.coupling(b),
            "KINEMATIC" => self.coupling_kind(b, CouplingKind::Kinematic),
            "DISTRIBUTING" => self.coupling_kind(b, CouplingKind::Distributing),
            "EQUATION" => self.equation(b),
            "MPC" => self.mpc(b),
            "END STEP" => self.end_step(b),
            _ => Err(b.error(b.line, "unsupported keyword")),
        }
    }

    fn node(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NSET"])?;
        self.model_data(b)?;
        let mut ids = Vec::new();
        for d in &b.data {
            if d.fields.len() < 3 {
                return Err(b.error(d.line, "node line needs an id and at least 2 coordinates"));
            }
            if d.fields.len() > 4 {
                return Err(b.error(
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
                return Err(b.error(d.line, format!("node {id} is defined twice")));
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
        let ty = b
            .value("TYPE")?
            .ok_or_else(|| b.error(b.line, "parameter TYPE is required"))?;
        let count = element_nodes(ty)
            .ok_or_else(|| b.error(b.line, format!("unsupported element type {ty}")))?;
        let mut ids = Vec::new();
        for d in &b.data {
            if d.fields.len() != count + 1 {
                return Err(b.error(
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
                    return Err(b.error(d.line_of(k), format!("node {n} is not defined")));
                }
                nodes.push(n);
            }
            if !self.element_ids.insert(id) {
                return Err(b.error(d.line, format!("element {id} is defined twice")));
            }
            self.model.elements.push(Element {
                id,
                element_type: ty.to_owned(),
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
        let name = b
            .value("NSET")?
            .ok_or_else(|| b.error(b.line, "parameter NSET is required"))?;
        let ids = self.set_members(b, true)?;
        self.add_nset(name, ids);
        Ok(())
    }

    fn elset(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["ELSET", "GENERATE"])?;
        self.model_data(b)?;
        let name = b
            .value("ELSET")?
            .ok_or_else(|| b.error(b.line, "parameter ELSET is required"))?;
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
                    return Err(b.error(d.line, "GENERATE needs first <= last and increment > 0"));
                }
                for id in (start..=end).step_by(step as usize) {
                    if !known.contains(&id) {
                        return Err(b.error(d.line, format!("{kind} {id} is not defined")));
                    }
                    out.push(id);
                }
                continue;
            }
            for f in d.fields.iter().filter(|f| !f.is_empty()) {
                if let Ok(id) = f.parse::<u32>() {
                    if !known.contains(&id) {
                        return Err(b.error(d.line, format!("{kind} {id} is not defined")));
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
                        return Err(b.error(d.line, format!("{kind} set {upper} is not defined")))
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
        let name = b
            .value("NAME")?
            .ok_or_else(|| b.error(b.line, "parameter NAME is required"))?;
        if let Some(d) = b.data.first() {
            return Err(b.error(d.line, "keyword takes no data lines"));
        }
        if self.model.material(name).is_some() {
            return Err(b.error(b.line, format!("material {name} is defined twice")));
        }
        self.model.materials.push(Material {
            name: name.to_owned(),
            elastic: None,
        });
        self.material = Some(self.model.materials.len() - 1);
        Ok(())
    }

    fn elastic(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["TYPE"])?;
        if let Some(t) = b.value("TYPE")? {
            if t != "ISOTROPIC" {
                return Err(b.error(b.line, format!("unsupported TYPE {t}")));
            }
        }
        let idx = self
            .material
            .ok_or_else(|| b.error(b.line, "keyword needs an open *MATERIAL"))?;
        let [d] = b.data.as_slice() else {
            return Err(b.error(b.line, "keyword needs exactly 1 data line"));
        };
        let elastic = Elastic {
            youngs_modulus: real(b, d, 0, "Young's modulus")?,
            poissons_ratio: real(b, d, 1, "Poisson's ratio")?,
        };
        if elastic.youngs_modulus <= 0.0 {
            return Err(b.error(d.line, "Young's modulus must be positive"));
        }
        // At -1 the shear modulus has no value: it divides by 1 + nu.
        if !(elastic.poissons_ratio > -1.0 && elastic.poissons_ratio < 0.5) {
            return Err(b.error(d.line, "Poisson's ratio must be in (-1, 0.5)"));
        }
        let mat = &mut self.model.materials[idx];
        if mat.elastic.is_some() {
            return Err(b.error(b.line, "material already has *ELASTIC"));
        }
        mat.elastic = Some(elastic);
        Ok(())
    }

    fn step(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["NAME", "NLGEOM"])?;
        if self.open_step.is_some() {
            return Err(b.error(b.line, "step inside a step"));
        }
        let nlgeom = match b.value("NLGEOM")? {
            None | Some("NO") => false,
            Some("YES") => true,
            Some(v) => return Err(b.error(b.line, format!("NLGEOM must be YES or NO, found {v}"))),
        };
        if let Some(d) = b.data.first() {
            return Err(b.error(d.line, "keyword takes no data lines"));
        }
        self.step_line = b.line;
        self.step_seen = true;
        self.open_step = Some(Step {
            name: b.value("NAME")?.map(str::to_owned),
            nlgeom,
            ..Step::default()
        });
        Ok(())
    }

    fn static_proc(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        let step = self
            .open_step
            .as_mut()
            .ok_or_else(|| b.error(b.line, "keyword needs an open *STEP"))?;
        if step.static_procedure.is_some() {
            return Err(b.error(b.line, "step already has *STATIC"));
        }
        let mut s = Static::default();
        if let Some(d) = b.data.first() {
            if b.data.len() > 1 || d.fields.len() > 4 {
                return Err(b.error(d.line, "keyword takes 1 data line with at most 4 values"));
            }
            s.initial_increment = opt_real(b, d, 0, "initial increment")?;
            s.time_period = opt_real(b, d, 1, "time period")?;
            s.min_increment = opt_real(b, d, 2, "minimum increment")?;
            s.max_increment = opt_real(b, d, 3, "maximum increment")?;
        }
        step.static_procedure = Some(s);
        Ok(())
    }

    fn target(&self, b: &Block, d: &DataLine) -> Result<Target, ParseError> {
        let f = d.fields.first().map(String::as_str).unwrap_or("");
        if let Ok(id) = f.parse::<u32>() {
            if !self.node_ids.contains(&id) {
                return Err(b.error(d.line, format!("node {id} is not defined")));
            }
            return Ok(Target::Node(id));
        }
        let name = f.to_uppercase();
        if name.is_empty() || self.model.nset(&name).is_none() {
            return Err(b.error(d.line, format!("node set '{name}' is not defined")));
        }
        Ok(Target::Set(name))
    }

    fn boundary(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() < 2 || d.fields.len() > 4 {
                return Err(b.error(
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
                return Err(b.error(d.line, "last dof is below first dof"));
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
            return Err(b.error(b.line, "keyword needs an open *STEP"));
        }
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() != 3 {
                return Err(b.error(d.line, "line needs node, dof and magnitude"));
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

    fn known_node(&self, b: &Block, line: usize, id: u32) -> Result<u32, ParseError> {
        if self.node_ids.contains(&id) {
            Ok(id)
        } else {
            Err(b.error(line, format!("node {id} is not defined")))
        }
    }

    /// Model data applies to all steps, so it must come before the first step starts.
    fn model_data(&self, b: &Block) -> Result<(), ParseError> {
        if self.step_seen {
            return Err(b.error(b.line, "keyword must come before the first *STEP"));
        }
        Ok(())
    }

    fn solid_section(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["ELSET", "MATERIAL"])?;
        self.model_data(b)?;
        let elset = b
            .value("ELSET")?
            .ok_or_else(|| b.error(b.line, "parameter ELSET is required"))?;
        let material = b
            .value("MATERIAL")?
            .ok_or_else(|| b.error(b.line, "parameter MATERIAL is required"))?;
        if let Some(d) = b.data.first() {
            return Err(b.error(d.line, "keyword takes no data lines"));
        }
        if self.model.elset(elset).is_none() {
            return Err(b.error(b.line, format!("element set '{elset}' is not defined")));
        }
        if self.model.material(material).is_none() {
            return Err(b.error(b.line, format!("material '{material}' is not defined")));
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
            Some("NODE") => {}
            Some(t) => return Err(b.error(b.line, format!("unsupported TYPE {t}"))),
            None => return Err(b.error(b.line, "parameter TYPE is required")),
        }
        let name = b
            .value("NAME")?
            .ok_or_else(|| b.error(b.line, "parameter NAME is required"))?;
        // A coupling finds its surface by name, so a second surface of a name is lost.
        if self.model.surfaces.iter().any(|s| s.name == name) {
            return Err(b.error(b.line, format!("surface '{name}' is already defined")));
        }
        let mut ids = Vec::new();
        for d in &b.data {
            if d.fields.len() != 1 {
                return Err(b.error(
                    d.line,
                    "line needs 1 node number or 1 node set name, without a weight",
                ));
            }
            match self.target(b, d)? {
                Target::Node(id) => ids.push(id),
                Target::Set(s) => ids.extend(self.model.nset(&s).map_or(&[][..], |n| &n.ids)),
            }
        }
        self.model.surfaces.push(NodeSurface {
            name: name.to_owned(),
            ids,
        });
        Ok(())
    }

    fn coupling(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&["CONSTRAINT NAME", "REF NODE", "SURFACE"])?;
        self.model_data(b)?;
        let name = b
            .value("CONSTRAINT NAME")?
            .ok_or_else(|| b.error(b.line, "parameter CONSTRAINT NAME is required"))?;
        let surface = b
            .value("SURFACE")?
            .ok_or_else(|| b.error(b.line, "parameter SURFACE is required"))?;
        let ref_text = b
            .value("REF NODE")?
            .ok_or_else(|| b.error(b.line, "parameter REF NODE is required"))?;
        let ref_id = ref_text.parse::<u32>().map_err(|_| {
            b.error(
                b.line,
                format!("REF NODE must be a node id, found '{ref_text}'"),
            )
        })?;
        let ref_node = self.known_node(b, b.line, ref_id)?;
        if !self.model.surfaces.iter().any(|s| s.name == surface) {
            return Err(b.error(b.line, format!("surface '{surface}' is not defined")));
        }
        if let Some(d) = b.data.first() {
            return Err(b.error(d.line, "keyword takes no data lines"));
        }
        self.open_coupling = Some(PendingCoupling {
            line: b.line,
            name: name.to_owned(),
            ref_node,
            surface: surface.to_owned(),
        });
        Ok(())
    }

    fn coupling_kind(&mut self, b: &Block, kind: CouplingKind) -> Result<(), ParseError> {
        b.allow(&[])?;
        let c = self
            .open_coupling
            .take()
            .ok_or_else(|| b.error(b.line, "keyword needs an open *COUPLING"))?;
        let (mut first_dof, mut last_dof) = (1, 6);
        if let Some(d) = b.data.first() {
            if b.data.len() > 1 || d.fields.len() > 2 {
                return Err(b.error(d.line, "keyword takes 1 data line: first dof, last dof"));
            }
            first_dof = dof(b, d, 0)?;
            last_dof = if d.fields.len() > 1 {
                dof(b, d, 1)?
            } else {
                first_dof
            };
            if last_dof < first_dof {
                return Err(b.error(d.line, "last dof is below first dof"));
            }
        }
        self.model.couplings.push(Coupling {
            name: c.name,
            ref_node: c.ref_node,
            surface: c.surface,
            kind,
            first_dof,
            last_dof,
        });
        Ok(())
    }

    /// Fail when a `*COUPLING` is open and the next keyword is not its kind.
    fn close_coupling(&mut self) -> Result<(), ParseError> {
        match self.open_coupling.take() {
            None => Ok(()),
            Some(c) => Err(ParseError::new(
                c.line,
                Some("COUPLING"),
                "needs *KINEMATIC or *DISTRIBUTING",
            )),
        }
    }

    fn equation(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        self.model_data(b)?;
        let first = b
            .data
            .first()
            .ok_or_else(|| b.error(b.line, "keyword needs data lines"))?;
        if first.fields.len() != 1 {
            return Err(b.error(first.line, "the first line gives only the number of terms"));
        }
        // Not `int`: a count of 0 gets its own message.
        let text = first.fields[0].as_str();
        let n = text.parse::<usize>().map_err(|_| {
            b.error(
                first.line,
                format!("number of terms must be a positive integer, found '{text}'"),
            )
        })?;
        if n == 0 {
            return Err(b.error(first.line, "equation needs at least 1 term"));
        }
        // Keep the empty fields: without them the next terms move to the wrong place.
        // The value check then refuses each empty field.
        let fields: Vec<(usize, &String)> = b.data[1..]
            .iter()
            .flat_map(|d| d.fields.iter().enumerate().map(|(j, f)| (d.line_of(j), f)))
            .collect();
        if fields.len() != 3 * n {
            return Err(b.error(
                first.line,
                format!("equation needs {n} terms of node, dof, coefficient"),
            ));
        }
        let mut terms = Vec::with_capacity(n);
        for t in fields.chunks(3) {
            let node = t[0].1.parse::<u32>().map_err(|_| {
                b.error(
                    t[0].0,
                    format!("node id must be a positive integer, found '{}'", t[0].1),
                )
            })?;
            let dof = t[1]
                .1
                .parse::<u8>()
                .ok()
                .filter(|d| (1..=6).contains(d))
                .ok_or_else(|| {
                    b.error(
                        t[1].0,
                        format!("degree of freedom must be 1 to 6, found '{}'", t[1].1),
                    )
                })?;
            let coefficient = finite(t[2].1).ok_or_else(|| {
                b.error(
                    t[2].0,
                    format!("coefficient must be a finite number, found '{}'", t[2].1),
                )
            })?;
            terms.push(EquationTerm {
                node: self.known_node(b, t[0].0, node)?,
                dof,
                coefficient,
            });
        }
        self.model.equations.push(Equation { terms });
        Ok(())
    }

    fn mpc(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        self.model_data(b)?;
        for d in &b.data {
            let kind = match d.fields[0].to_uppercase().as_str() {
                "TIE" => MpcKind::Tie,
                "PIN" => MpcKind::Pin,
                "LINK" => MpcKind::Link,
                "BEAM" => MpcKind::Beam,
                t => return Err(b.error(d.line, format!("unsupported MPC type {t}"))),
            };
            if d.fields.len() != 3 {
                return Err(b.error(d.line, format!("{} needs 2 nodes", d.fields[0])));
            }
            let mut nodes = Vec::new();
            for k in 1..3 {
                let id = int(b, d, k, "node id")?;
                nodes.push(self.known_node(b, d.line_of(k), id)?);
            }
            self.model.mpcs.push(Mpc { kind, nodes });
        }
        Ok(())
    }

    fn dload(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if self.open_step.is_none() {
            return Err(b.error(b.line, "keyword needs an open *STEP"));
        }
        let mut out = Vec::new();
        for d in &b.data {
            if d.fields.len() != 3 {
                return Err(b.error(d.line, "line needs element, load type and magnitude"));
            }
            let ty = d.fields[1].as_str();
            let face = ty
                .strip_prefix(['P', 'p'])
                .and_then(|n| n.parse::<u8>().ok())
                .ok_or_else(|| b.error(d.line, format!("unsupported load type {ty}")))?;
            if !(1..=6).contains(&face) {
                return Err(b.error(d.line, format!("face must be 1 to 6, found {face}")));
            }
            let f = d.fields[0].as_str();
            let target = if let Ok(id) = f.parse::<u32>() {
                if !self.element_ids.contains(&id) {
                    return Err(b.error(d.line, format!("element {id} is not defined")));
                }
                ElementTarget::Element(id)
            } else {
                let name = f.to_uppercase();
                if self.model.elset(&name).is_none() {
                    return Err(b.error(d.line, format!("element set '{name}' is not defined")));
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

    fn end_step(&mut self, b: &Block) -> Result<(), ParseError> {
        b.allow(&[])?;
        if let Some(d) = b.data.first() {
            return Err(b.error(d.line, "keyword takes no data lines"));
        }
        let step = self
            .open_step
            .take()
            .ok_or_else(|| b.error(b.line, "no open *STEP"))?;
        if step.static_procedure.is_none() {
            return Err(b.error(self.step_line, "step has no procedure (*STATIC)"));
        }
        self.model.steps.push(step);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESH: &str =
        "*NODE\n1,0,0,0\n2,1,0,0\n3,1,1,0\n4,0,1,0\n5,0,0,1\n6,1,0,1\n7,1,1,1\n8,0,1,1\n";

    fn err(text: &str) -> String {
        parse(text).unwrap_err().to_string()
    }

    #[test]
    fn unsupported_keyword_names_line_and_keyword() {
        assert_eq!(
            err("*NODE\n1,0,0,0\n*PLASTIC\n"),
            "line 3, *PLASTIC: unsupported keyword"
        );
    }

    #[test]
    fn part_and_contact_keywords_are_rejected() {
        for k in [
            "PART, NAME=A",
            "ASSEMBLY, NAME=A",
            "CONTACT PAIR",
            "SURFACE INTERACTION, NAME=A",
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
            // Line 13 is the second physical line of the record.
            (
                "*EQUATION\n2\n1,1,1,\n2,7,-1\n",
                "line 13, *EQUATION: degree of freedom",
            ),
            (
                "*EQUATION\n2\n1,1,1,\n2,1,x\n",
                "line 13, *EQUATION: coefficient",
            ),
            (
                "*EQUATION\n2\n1,1,1,\n99,1,-1\n",
                "line 13, *EQUATION: node 99",
            ),
            (
                "*ELEMENT, TYPE=C3D8\n1,1,2,3,4,\n5,6,7,99\n",
                "line 12, *ELEMENT: node 99",
            ),
            (
                "*ELEMENT, TYPE=C3D8\n1,1,2,3,4,\n5,6,7,x\n",
                "line 12, *ELEMENT: node id",
            ),
            ("*NODE\n9,0,\n0,x\n", "line 12, *NODE: "),
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
            "line 4, *END STEP: keyword takes no data lines"
        );
    }

    #[test]
    fn a_zero_label_is_an_error() {
        assert_eq!(
            err("*NODE\n0,0,0,0\n"),
            "line 2, *NODE: node id must be a positive integer, found '0'"
        );
        let element = format!("{MESH}*ELEMENT, TYPE=C3D8\n0,1,2,3,4,5,6,7,8\n");
        assert_eq!(
            err(&element),
            "line 11, *ELEMENT: element id must be a positive integer, found '0'"
        );
    }

    #[test]
    fn element_errors_name_the_line() {
        let short = format!("{MESH}*ELEMENT, TYPE=C3D8\n1,1,2,3\n");
        assert!(err(&short).starts_with("line 11, *ELEMENT: C3D8 needs an id and 8 nodes"));
        let missing = format!("{MESH}*ELEMENT, TYPE=C3D8\n1,1,2,3,4,5,6,7,99\n");
        assert_eq!(err(&missing), "line 11, *ELEMENT: node 99 is not defined");
        let ty = format!("{MESH}*ELEMENT, TYPE=S4R\n");
        assert_eq!(err(&ty), "line 10, *ELEMENT: unsupported element type S4R");
    }

    #[test]
    fn bad_number_names_line() {
        assert_eq!(
            err("*NODE\n1,0,x,0\n"),
            "line 2, *NODE: coordinate must be a finite number, found 'x'"
        );
    }

    #[test]
    fn duplicate_node_fails() {
        assert_eq!(
            err("*NODE\n1,0,0,0\n1,1,0,0\n"),
            "line 3, *NODE: node 1 is defined twice"
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
            "line 1, *NODE: unsupported parameter FOO"
        );
    }

    #[test]
    fn elastic_outside_material_fails() {
        assert_eq!(
            err("*ELASTIC\n1, 0.3\n"),
            "line 1, *ELASTIC: keyword needs an open *MATERIAL"
        );
        assert!(err("*MATERIAL, NAME=A\n*NODE\n*ELASTIC\n1,0.3\n").contains("line 3, *ELASTIC"));
    }

    #[test]
    fn step_rules() {
        assert_eq!(err("*STEP\n"), "line 1, *STEP: step has no *END STEP");
        assert_eq!(
            err("*STEP\n*END STEP\n"),
            "line 1, *END STEP: step has no procedure (*STATIC)"
        );
        assert_eq!(
            err("*STATIC\n"),
            "line 1, *STATIC: keyword needs an open *STEP"
        );
        assert_eq!(err("*STEP\n*STEP\n"), "line 2, *STEP: step inside a step");
        assert_eq!(
            err("*CLOAD\n"),
            "line 1, *CLOAD: keyword needs an open *STEP"
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
            "{MESH}*MATERIAL, NAME=M\n*ELASTIC, TYPE=ISOTROPIC\n100, 0.3\n*STEP, NAME=S, NLGEOM=NO\n*STATIC\n0.1, 1\n*CLOAD\n8, 3, -2.5\n*END STEP\n"
        ))
        .unwrap();
        let s = &m.steps[0];
        assert!(!s.nlgeom);
        assert_eq!(s.static_procedure.as_ref().unwrap().time_period, Some(1.0));
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
    fn elastic_range_checks() {
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n100, 0.5\n").contains("Poisson's ratio"));
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n100, -1\n").contains("Poisson's ratio"));
        assert!(err("*MATERIAL, NAME=M\n*ELASTIC\n-1, 0.3\n").contains("Young's modulus"));
    }
}
