use std::collections::{BTreeMap, HashSet};

use crate::{
    Boundary, Cload, Elastic, Element, ElementKind, Material, Model, Node, ParseError, Static,
    Step, Target,
};

/// Reads `.inp` text. Keywords are case-insensitive. Lines starting with
/// `**` are comments.
pub fn parse(text: &str) -> Result<Model, ParseError> {
    let mut reader = Reader {
        keyword: "<none>".to_string(),
        ..Reader::default()
    };
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("**") {
            continue;
        }
        let result = match trimmed.strip_prefix('*') {
            Some(rest) => reader.keyword_line(rest),
            None => reader.data_line(trimmed),
        };
        if let Err(message) = result {
            return Err(ParseError {
                line,
                keyword: reader.keyword.clone(),
                message,
            });
        }
    }
    Ok(reader.model)
}

#[derive(Debug, Clone, PartialEq)]
struct Param {
    key: String,
    value: Option<String>,
}

/// The keyword whose data lines come next.
#[derive(Debug, Clone, Default)]
enum State {
    #[default]
    Idle,
    Node,
    Element {
        kind: ElementKind,
        elset: Option<String>,
    },
    Set {
        node: bool,
        name: String,
        generate: bool,
    },
    Elastic(usize),
    Static(usize),
    Boundary(usize),
    Cload(usize),
    /// A keyword that takes one data line has read it. More data lines fail.
    Spent,
}

#[derive(Default)]
struct Reader {
    model: Model,
    node_ids: HashSet<u32>,
    element_ids: HashSet<u32>,
    current_material: Option<usize>,
    current_step: Option<usize>,
    state: State,
    keyword: String,
}

impl Reader {
    fn keyword_line(&mut self, rest: &str) -> Result<(), String> {
        let mut parts = rest.split(',');
        let name = parts.next().unwrap_or("").trim().to_ascii_uppercase();
        let params = params(parts)?;
        self.keyword = format!("*{name}");
        self.state = State::Idle;
        match name.as_str() {
            "NODE" => {
                check(&params, &[])?;
                self.state = State::Node;
            }
            "ELEMENT" => self.element_keyword(&params)?,
            "NSET" => self.set_keyword(&params, true)?,
            "ELSET" => self.set_keyword(&params, false)?,
            "MATERIAL" => self.material_keyword(&params)?,
            "ELASTIC" => self.elastic_keyword(&params)?,
            "STEP" => self.step_keyword(&params)?,
            "END STEP" => {
                check(&params, &[])?;
                if self.current_step.take().is_none() {
                    return Err("no open *STEP to end".into());
                }
            }
            "STATIC" => {
                check(&params, &[])?;
                let step = self.open_step()?;
                self.model.steps[step].statics = Some(Static::default());
                self.state = State::Static(step);
            }
            "BOUNDARY" => {
                check(&params, &[])?;
                self.state = State::Boundary(self.open_step()?);
            }
            "CLOAD" => {
                check(&params, &[])?;
                self.state = State::Cload(self.open_step()?);
            }
            _ => return Err("keyword is not supported by the inp reader in M1".into()),
        }
        Ok(())
    }

    fn data_line(&mut self, text: &str) -> Result<(), String> {
        let mut fields: Vec<&str> = text.split(',').map(str::trim).collect();
        while fields.last() == Some(&"") {
            fields.pop();
        }
        match self.state.clone() {
            State::Idle | State::Spent => Err("data line is not allowed here".into()),
            State::Node => self.node_data(&fields),
            State::Element { kind, elset } => self.element_data(kind, elset, &fields),
            State::Set {
                node,
                name,
                generate,
            } => self.set_data(node, name, generate, &fields),
            State::Elastic(material) => {
                if fields.len() != 2 {
                    return Err("data line needs a Young's modulus and a Poisson ratio".into());
                }
                self.model.materials[material].elastic = Some(Elastic {
                    young: number(fields[0])?,
                    poisson: number(fields[1])?,
                });
                self.state = State::Spent;
                Ok(())
            }
            State::Static(step) => {
                if fields.is_empty() || fields.len() > 4 {
                    return Err("data line needs 1 to 4 values".into());
                }
                let mut values: [Option<f64>; 4] = [None; 4];
                for (slot, field) in values.iter_mut().zip(fields.iter()) {
                    *slot = Some(number(field)?);
                }
                self.model.steps[step].statics = Some(Static {
                    initial_inc: values[0],
                    period: values[1],
                    min_inc: values[2],
                    max_inc: values[3],
                });
                self.state = State::Spent;
                Ok(())
            }
            State::Boundary(step) => self.boundary_data(step, &fields),
            State::Cload(step) => self.cload_data(step, &fields),
        }
    }

    fn element_keyword(&mut self, params: &[Param]) -> Result<(), String> {
        check(params, &["TYPE", "ELSET"])?;
        let type_name = value_of(params, "TYPE")?
            .ok_or_else(|| "needs TYPE=element type".to_string())?
            .to_ascii_uppercase();
        let kind = ElementKind::from_name(&type_name)
            .ok_or_else(|| format!("element type {type_name} is not supported"))?;
        let elset = name_of(params, "ELSET")?;
        self.state = State::Element { kind, elset };
        Ok(())
    }

    fn set_keyword(&mut self, params: &[Param], node: bool) -> Result<(), String> {
        let key = if node { "NSET" } else { "ELSET" };
        check(params, &[key, "GENERATE"])?;
        let name = name_of(params, key)?.ok_or_else(|| format!("needs {key}=name"))?;
        let generate = params.iter().any(|p| p.key == "GENERATE");
        self.sets(node).entry(name.clone()).or_default();
        self.state = State::Set {
            node,
            name,
            generate,
        };
        Ok(())
    }

    fn material_keyword(&mut self, params: &[Param]) -> Result<(), String> {
        check(params, &["NAME"])?;
        let name = name_of(params, "NAME")?.ok_or_else(|| "needs NAME=name".to_string())?;
        if self.model.materials.iter().any(|m| m.name == name) {
            return Err(format!("material {name} is already defined"));
        }
        self.model.materials.push(Material {
            name,
            elastic: None,
        });
        self.current_material = Some(self.model.materials.len() - 1);
        Ok(())
    }

    fn elastic_keyword(&mut self, params: &[Param]) -> Result<(), String> {
        check(params, &["TYPE"])?;
        if let Some(kind) = value_of(params, "TYPE")? {
            if !kind.eq_ignore_ascii_case("ISOTROPIC") {
                return Err(format!(
                    "elastic type {kind} is not supported, use ISOTROPIC"
                ));
            }
        }
        let material = self
            .current_material
            .ok_or_else(|| "needs a *MATERIAL before it".to_string())?;
        self.state = State::Elastic(material);
        Ok(())
    }

    fn step_keyword(&mut self, params: &[Param]) -> Result<(), String> {
        check(params, &["NAME", "NLGEOM"])?;
        if self.current_step.is_some() {
            return Err("a *STEP is already open, close it with *END STEP".into());
        }
        let nlgeom = match value_of(params, "NLGEOM")? {
            None => false,
            Some(v) if v.eq_ignore_ascii_case("NO") => false,
            Some(v) if v.eq_ignore_ascii_case("YES") => true,
            Some(v) => return Err(format!("NLGEOM={v} is not YES or NO")),
        };
        let name = name_of(params, "NAME")?;
        self.model.steps.push(Step {
            name,
            nlgeom,
            statics: None,
            boundaries: Vec::new(),
            cloads: Vec::new(),
        });
        self.current_step = Some(self.model.steps.len() - 1);
        Ok(())
    }

    fn open_step(&self) -> Result<usize, String> {
        self.current_step
            .ok_or_else(|| "must be inside a *STEP".to_string())
    }

    fn sets(&mut self, node: bool) -> &mut BTreeMap<String, Vec<u32>> {
        if node {
            &mut self.model.nsets
        } else {
            &mut self.model.elsets
        }
    }

    fn node_data(&mut self, fields: &[&str]) -> Result<(), String> {
        if fields.len() != 3 && fields.len() != 4 {
            return Err("node line needs an id, x, y and optionally z".into());
        }
        let id = uint(fields[0])?;
        let mut coords = [0.0; 3];
        for (slot, field) in coords.iter_mut().zip(fields[1..].iter()) {
            *slot = number(field)?;
        }
        if !self.node_ids.insert(id) {
            return Err(format!("node {id} is already defined"));
        }
        self.model.nodes.push(Node { id, coords });
        Ok(())
    }

    fn element_data(
        &mut self,
        kind: ElementKind,
        elset: Option<String>,
        fields: &[&str],
    ) -> Result<(), String> {
        let needed = kind.node_count();
        if fields.len() != needed + 1 {
            return Err(format!(
                "{} needs {} nodes after the id, found {}",
                kind.name(),
                needed,
                fields.len().saturating_sub(1)
            ));
        }
        let id = uint(fields[0])?;
        let nodes = fields[1..]
            .iter()
            .map(|field| uint(field))
            .collect::<Result<Vec<u32>, String>>()?;
        if !self.element_ids.insert(id) {
            return Err(format!("element {id} is already defined"));
        }
        if let Some(name) = elset {
            self.model.elsets.entry(name).or_default().push(id);
        }
        self.model.elements.push(Element { id, kind, nodes });
        Ok(())
    }

    fn set_data(
        &mut self,
        node: bool,
        name: String,
        generate: bool,
        fields: &[&str],
    ) -> Result<(), String> {
        if generate {
            if fields.len() != 3 {
                return Err("GENERATE needs start, end and increment".into());
            }
            let (start, end, step) = (uint(fields[0])?, uint(fields[1])?, uint(fields[2])?);
            if step == 0 || start > end {
                return Err("GENERATE needs an increment above 0 and start not above end".into());
            }
            let ids = (start..=end).step_by(step as usize);
            self.sets(node).entry(name).or_default().extend(ids);
        } else {
            let ids = fields
                .iter()
                .map(|field| uint(field))
                .collect::<Result<Vec<u32>, String>>()?;
            self.sets(node).entry(name).or_default().extend(ids);
        }
        Ok(())
    }

    fn boundary_data(&mut self, step: usize, fields: &[&str]) -> Result<(), String> {
        if !(2..=4).contains(&fields.len()) {
            return Err(
                "data line needs a node or set, a first dof, and optionally a last dof and a value"
                    .into(),
            );
        }
        let target = target(fields[0])?;
        let first_dof = dof(fields[1])?;
        let last_dof = match fields.get(2) {
            Some(field) => dof(field)?,
            None => first_dof,
        };
        if last_dof < first_dof {
            return Err("last dof is below first dof".into());
        }
        let value = match fields.get(3) {
            Some(field) => number(field)?,
            None => 0.0,
        };
        self.model.steps[step].boundaries.push(Boundary {
            target,
            first_dof,
            last_dof,
            value,
        });
        Ok(())
    }

    fn cload_data(&mut self, step: usize, fields: &[&str]) -> Result<(), String> {
        if fields.len() != 3 {
            return Err("data line needs a node or set, a dof, and a magnitude".into());
        }
        let load = Cload {
            target: target(fields[0])?,
            dof: dof(fields[1])?,
            magnitude: number(fields[2])?,
        };
        self.model.steps[step].cloads.push(load);
        Ok(())
    }
}

/// Splits the parameters after a keyword name. A flag has no value.
fn params<'a>(parts: impl Iterator<Item = &'a str>) -> Result<Vec<Param>, String> {
    let mut out = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let param = match part.split_once('=') {
            Some((key, value)) => Param {
                key: key.trim().to_ascii_uppercase(),
                value: Some(value.trim().to_ascii_uppercase()),
            },
            None => Param {
                key: part.to_ascii_uppercase(),
                value: None,
            },
        };
        out.push(param);
    }
    Ok(out)
}

/// Fails on any parameter not in `allowed`.
fn check(params: &[Param], allowed: &[&str]) -> Result<(), String> {
    match params.iter().find(|p| !allowed.contains(&p.key.as_str())) {
        Some(p) => Err(format!("unknown parameter {}", p.key)),
        None => Ok(()),
    }
}

/// The value of a parameter. A parameter with no value is an error.
fn value_of<'a>(params: &'a [Param], key: &str) -> Result<Option<&'a str>, String> {
    match params.iter().find(|p| p.key == key) {
        None => Ok(None),
        Some(Param {
            value: Some(value), ..
        }) => Ok(Some(value.as_str())),
        Some(_) => Err(format!("{key} needs a value")),
    }
}

fn name_of(params: &[Param], key: &str) -> Result<Option<String>, String> {
    Ok(value_of(params, key)?.map(str::to_owned))
}

fn uint(field: &str) -> Result<u32, String> {
    field
        .parse()
        .map_err(|_| format!("expected an integer, found {field:?}"))
}

fn number(field: &str) -> Result<f64, String> {
    field
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("expected a number, found {field:?}"))
}

fn dof(field: &str) -> Result<u8, String> {
    match field.parse::<u8>() {
        Ok(d) if (1..=6).contains(&d) => Ok(d),
        _ => Err(format!("expected a dof from 1 to 6, found {field:?}")),
    }
}

/// A node id, or a set name that starts with a letter.
fn target(field: &str) -> Result<Target, String> {
    if let Ok(id) = field.parse::<u32>() {
        return Ok(Target::Node(id));
    }
    let mut chars = field.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let valid = starts_with_letter && field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(Target::Set(field.to_ascii_uppercase()))
    } else {
        Err(format!("{field:?} is not a node id or a set name"))
    }
}
