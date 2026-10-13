//! Reader for the gmsh MSH 4.1 ASCII format. It builds the `inp` model.
//! Decision D-02, see DESIGN.md: gmsh stays external and is never linked.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use crate::code;
use fh_inp::{Element, ElementSet, Model, Node, NodeSet};

/// An import error. It has a stable code and names the line number of the input.
/// `file` is empty until the caller sets it with `with_file`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MshError {
    pub code: &'static str,
    pub file: Option<String>,
    pub line: usize,
    pub message: String,
}

impl MshError {
    /// The same error with the name of the file that holds the line.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

impl fmt::Display for MshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{file}: ")?;
        }
        write!(f, "line {}: {} ({})", self.line, self.message, self.code)
    }
}

impl std::error::Error for MshError {}

type Result<T> = std::result::Result<T, MshError>;

fn fail<T>(code: &'static str, line: usize, message: impl Into<String>) -> Result<T> {
    Err(MshError {
        code,
        file: None,
        line,
        message: message.into(),
    })
}

/// Node count and dimension of each gmsh element type that can occur in a mesh.
fn type_info(t: u32) -> Option<(usize, u8)> {
    Some(match t {
        15 => (1, 0),
        1 => (2, 1),
        8 => (3, 1),
        2 => (3, 2),
        3 => (4, 2),
        9 => (6, 2),
        10 => (9, 2),
        16 => (8, 2),
        4 => (4, 3),
        5 => (8, 3),
        6 => (6, 3),
        7 => (5, 3),
        11 => (10, 3),
        _ => return None,
    })
}

/// The `inp` element type and the gmsh position of each `inp` node.
fn inp_type(t: u32) -> Option<(&'static str, &'static [usize])> {
    match t {
        5 => Some(("C3D8I", &[0, 1, 2, 3, 4, 5, 6, 7])),
        // gmsh puts the last two midside nodes in the other order.
        11 => Some(("C3D10", &[0, 1, 2, 3, 4, 5, 6, 7, 9, 8])),
        _ => None,
    }
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Lines<'a> {
    /// The next non-blank line, as its number and tokens.
    fn next(&mut self) -> Result<(usize, Vec<&'a str>)> {
        while let Some(text) = self.lines.get(self.pos) {
            self.pos += 1;
            let tokens: Vec<&str> = text.split_whitespace().collect();
            if !tokens.is_empty() {
                return Ok((self.pos, tokens));
            }
        }
        fail(code::MSH_SYNTAX, self.pos + 1, "unexpected end of file")
    }
}

fn num<T: FromStr>(tokens: &[&str], i: usize, line: usize, what: &str) -> Result<T> {
    match tokens.get(i) {
        None => fail(code::MSH_SYNTAX, line, format!("missing {what}")),
        Some(t) => t.parse().or_else(|_| {
            fail(
                code::MSH_SYNTAX,
                line,
                format!("{what} is not a valid number: {t}"),
            )
        }),
    }
}

/// A node or element tag. It becomes a label of the model, and a label starts at 1.
fn label(tokens: &[&str], i: usize, line: usize, what: &str) -> Result<u32> {
    match num(tokens, i, line, what)? {
        0 => fail(
            code::MSH_SYNTAX,
            line,
            format!("{what} 0 is not valid, a tag starts at 1"),
        ),
        id => Ok(id),
    }
}

/// A coordinate. The Rust parser also accepts NaN, infinity and numbers that overflow.
fn coordinate(tokens: &[&str], i: usize, line: usize, what: &str) -> Result<f64> {
    let v: f64 = num(tokens, i, line, what)?;
    if v.is_finite() {
        Ok(v)
    } else {
        fail(
            code::MSH_SYNTAX,
            line,
            format!("{what} is not a finite number: {}", tokens[i]),
        )
    }
}

#[derive(Default)]
struct Group {
    name: String,
    nodes: Vec<u32>,
    elements: Vec<u32>,
}

#[derive(Default)]
struct Reader {
    names: HashMap<(u8, u32), String>,
    /// Physical tags of each geometric entity.
    entities: HashMap<(u8, u32), Vec<u32>>,
    model: Model,
    node_ids: HashSet<u32>,
    element_ids: HashSet<u32>,
    groups: Vec<Group>,
}

/// Read a gmsh MSH 4.1 ASCII file. Each physical group becomes a node set.
/// A group of volumes also becomes an element set.
pub fn read_msh(text: &str) -> Result<Model> {
    let mut l = Lines {
        lines: text.lines().collect(),
        pos: 0,
    };
    let mut r = Reader::default();
    let (line, t) = l.next()?;
    if t != ["$MeshFormat"] {
        return fail(code::MSH_SYNTAX, line, "expected $MeshFormat");
    }
    r.format(&mut l)?;
    while l.pos < l.lines.len() {
        let Ok((line, t)) = l.next() else { break };
        let Some(name) = t[0].strip_prefix('$') else {
            return fail(
                code::MSH_SYNTAX,
                line,
                format!("expected a section start, found {}", t[0]),
            );
        };
        match name {
            "PhysicalNames" => r.physical_names(&mut l)?,
            "Entities" => r.read_entities(&mut l)?,
            "Nodes" => r.nodes(&mut l)?,
            "Elements" => r.elements(&mut l)?,
            _ => skip(&mut l, name)?,
        }
        let (line, t) = l.next()?;
        if t != [format!("$End{name}")] {
            return fail(code::MSH_SYNTAX, line, format!("expected $End{name}"));
        }
    }
    r.finish()
}

fn skip(l: &mut Lines, name: &str) -> Result<()> {
    let end = format!("$End{name}");
    loop {
        let (_, t) = l.next()?;
        if t[0] == end {
            l.pos -= 1;
            return Ok(());
        }
    }
}

impl Reader {
    fn format(&mut self, l: &mut Lines) -> Result<()> {
        let (line, t) = l.next()?;
        if t[0] != "4.1" {
            return fail(
                code::MSH_UNSUPPORTED,
                line,
                format!("unsupported MSH version {}, expected 4.1", t[0]),
            );
        }
        if num::<u8>(&t, 1, line, "file type")? != 0 {
            return fail(
                code::MSH_UNSUPPORTED,
                line,
                "binary MSH is not supported, write ASCII",
            );
        }
        let (line, t) = l.next()?;
        if t != ["$EndMeshFormat"] {
            return fail(code::MSH_SYNTAX, line, "expected $EndMeshFormat");
        }
        Ok(())
    }

    fn physical_names(&mut self, l: &mut Lines) -> Result<()> {
        let (line, t) = l.next()?;
        for _ in 0..num::<usize>(&t, 0, line, "name count")? {
            let (line, t) = l.next()?;
            let dim: u8 = num(&t, 0, line, "dimension")?;
            let tag: u32 = num(&t, 1, line, "physical tag")?;
            let text = l.lines[line - 1];
            let (Some(a), Some(b)) = (text.find('"'), text.rfind('"')) else {
                return fail(
                    code::MSH_SYNTAX,
                    line,
                    "the physical name needs quotation marks",
                );
            };
            if a == b {
                return fail(
                    code::MSH_SYNTAX,
                    line,
                    "the physical name needs quotation marks",
                );
            }
            self.names.insert((dim, tag), text[a + 1..b].to_owned());
        }
        Ok(())
    }

    fn read_entities(&mut self, l: &mut Lines) -> Result<()> {
        let (line, t) = l.next()?;
        let mut counts = [0usize; 4];
        for (d, c) in counts.iter_mut().enumerate() {
            *c = num(&t, d, line, "entity count")?;
        }
        for (dim, count) in counts.iter().enumerate() {
            for _ in 0..*count {
                let (line, t) = l.next()?;
                let tag: u32 = num(&t, 0, line, "entity tag")?;
                let at = if dim == 0 { 4 } else { 7 };
                let n: usize = num(&t, at, line, "physical tag count")?;
                let mut tags = Vec::new();
                for i in 0..n {
                    tags.push(num::<i32>(&t, at + 1 + i, line, "physical tag")?.unsigned_abs());
                }
                self.entities.insert((dim as u8, tag), tags);
            }
        }
        Ok(())
    }

    fn nodes(&mut self, l: &mut Lines) -> Result<()> {
        let (line, t) = l.next()?;
        for _ in 0..num::<usize>(&t, 0, line, "block count")? {
            let (line, t) = l.next()?;
            let dim: u8 = num(&t, 0, line, "entity dimension")?;
            let tag: u32 = num(&t, 1, line, "entity tag")?;
            let n: usize = num(&t, 3, line, "node count")?;
            let mut ids = Vec::new();
            for _ in 0..n {
                let (line, t) = l.next()?;
                let id = label(&t, 0, line, "node tag")?;
                if !self.node_ids.insert(id) {
                    return fail(
                        code::MSH_DUPLICATE,
                        line,
                        format!("node {id} is defined twice"),
                    );
                }
                ids.push(id);
            }
            for id in &ids {
                let (line, t) = l.next()?;
                let xyz = [
                    coordinate(&t, 0, line, "x")?,
                    coordinate(&t, 1, line, "y")?,
                    coordinate(&t, 2, line, "z")?,
                ];
                self.model.nodes.push(Node { id: *id, xyz });
            }
            if dim == 0 {
                self.add_to_groups(dim, tag, &ids, &[]);
            }
        }
        Ok(())
    }

    fn elements(&mut self, l: &mut Lines) -> Result<()> {
        let (line, t) = l.next()?;
        for _ in 0..num::<usize>(&t, 0, line, "block count")? {
            let (line, t) = l.next()?;
            let dim: u8 = num(&t, 0, line, "entity dimension")?;
            let tag: u32 = num(&t, 1, line, "entity tag")?;
            let ty: u32 = num(&t, 2, line, "element type")?;
            let n: usize = num(&t, 3, line, "element count")?;
            let Some((count, tdim)) = type_info(ty) else {
                return fail(
                    code::MSH_UNSUPPORTED,
                    line,
                    format!("unknown element type {ty}"),
                );
            };
            let inp = inp_type(ty);
            if tdim == 3 && inp.is_none() {
                return fail(
                    code::MSH_UNSUPPORTED,
                    line,
                    format!("element type {ty} is not accepted, use 5 or 11"),
                );
            }
            for _ in 0..n {
                let (line, t) = l.next()?;
                let id = label(&t, 0, line, "element tag")?;
                if t.len() != count + 1 {
                    return fail(
                        code::MSH_SYNTAX,
                        line,
                        format!(
                            "element type {ty} needs {count} nodes, found {}",
                            t.len() - 1
                        ),
                    );
                }
                if !self.element_ids.insert(id) {
                    return fail(
                        code::MSH_DUPLICATE,
                        line,
                        format!("element {id} is defined twice"),
                    );
                }
                let mut nodes = Vec::new();
                for i in 1..=count {
                    let node: u32 = num(&t, i, line, "node tag")?;
                    if !self.node_ids.contains(&node) {
                        return fail(
                            code::MSH_UNDEFINED,
                            line,
                            format!("element {id} uses unknown node {node}"),
                        );
                    }
                    nodes.push(node);
                }
                let mut elements = Vec::new();
                if let Some((name, order)) = inp {
                    elements.push(id);
                    self.model.elements.push(Element {
                        id,
                        element_type: name.to_owned(),
                        nodes: order.iter().map(|&i| nodes[i]).collect(),
                    });
                }
                self.add_to_groups(dim, tag, &nodes, &elements);
            }
        }
        Ok(())
    }

    fn add_to_groups(&mut self, dim: u8, tag: u32, nodes: &[u32], elements: &[u32]) {
        for phys in self.entities.get(&(dim, tag)).cloned().unwrap_or_default() {
            let name = self
                .names
                .get(&(dim, phys))
                .cloned()
                .unwrap_or_else(|| format!("PHYS_{dim}_{phys}"));
            let i = match self.groups.iter().position(|g| g.name == name) {
                Some(i) => i,
                None => {
                    self.groups.push(Group {
                        name,
                        ..Group::default()
                    });
                    self.groups.len() - 1
                }
            };
            self.groups[i].nodes.extend_from_slice(nodes);
            self.groups[i].elements.extend_from_slice(elements);
        }
    }

    fn finish(mut self) -> Result<Model> {
        for g in self.groups {
            let (mut nodes, mut elements) = (g.nodes, g.elements);
            nodes.sort_unstable();
            nodes.dedup();
            elements.sort_unstable();
            elements.dedup();
            if !elements.is_empty() {
                self.model.elsets.push(ElementSet {
                    name: g.name.clone(),
                    ids: elements,
                });
            }
            if !nodes.is_empty() {
                self.model.nsets.push(NodeSet {
                    name: g.name,
                    ids: nodes,
                });
            }
        }
        Ok(self.model)
    }
}
