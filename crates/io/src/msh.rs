//! Reader for the gmsh MSH 4.1 ASCII format. It builds the `inp` model.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;

use crate::code;
use fh_inp::{Element, ElementSet, ElementSurface, Model, Node, NodeSet};

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

/// The sides of an element that a `*SURFACE, TYPE=ELEMENT` names: the number of each side and
/// its corner positions (in `inp` node order). A solid has faces `S1` to `S4`. A shell has its
/// edges as `S3` and up: edge 1 joins corner 1 and corner 2.
fn sides(element_type: &str) -> &'static [(u8, &'static [usize])] {
    match element_type {
        "C3D10" => &[
            (1, &[0, 1, 2]),
            (2, &[0, 3, 1]),
            (3, &[1, 3, 2]),
            (4, &[2, 3, 0]),
        ],
        "S6" => &[(3, &[0, 1]), (4, &[1, 2]), (5, &[2, 0])],
        _ => &[],
    }
}

/// The `inp` element type and the gmsh position of each `inp` node. A solid mesh accepts the
/// 10-node tet (gmsh type 11). A shell mesh accepts the 6-node triangle (type 9).
fn inp_type(t: u32, shell: bool) -> Option<(&'static str, &'static [usize])> {
    match (t, shell) {
        // gmsh puts the last two midside nodes in the other order.
        (11, false) => Some(("C3D10", &[0, 1, 2, 3, 4, 5, 6, 7, 9, 8])),
        (9, true) => Some(("S6", &[0, 1, 2, 3, 4, 5])),
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
    /// The sorted corner nodes of each face element (a triangle or a quadrangle), or of each
    /// edge element in a shell mesh.
    faces: Vec<Vec<u32>>,
}

/// A mesh, with the facts about its physical groups that the model does not hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MshMesh {
    pub model: Model,
    /// Tags of the volumes (the surfaces in a shell mesh) with no physical group, in a file with
    /// at least one physical group. gmsh then writes no element of such a body, so that part is
    /// lost. A file with no physical group holds every element, and the list is empty.
    pub unnamed_volumes: Vec<u32>,
    /// The file has a volume. A shell mesh must not have one.
    pub has_volume: bool,
    /// Physical surface groups (curve groups in a shell mesh) that are not on the outside of the
    /// body: a side of the group is a side of no element, or of two. They get a node set, but no
    /// element surface.
    pub inner_surfaces: Vec<String>,
}

#[derive(Default)]
struct Reader {
    /// A shell mesh: the elements are surface elements, and the sides are curve elements.
    shell: bool,
    names: HashMap<(u8, u32), String>,
    /// Physical tags of each geometric entity.
    entities: HashMap<(u8, u32), Vec<u32>>,
    model: Model,
    node_ids: HashSet<u32>,
    element_ids: HashSet<u32>,
    groups: Vec<Group>,
    /// Each volume entity (surface entity in a shell mesh), and whether it has a physical tag.
    volumes: Vec<(u32, bool)>,
    has_volume: bool,
}

/// Read a gmsh MSH 4.1 ASCII file. Each physical group becomes a node set.
/// A group of volumes also becomes an element set. A group of surfaces on the outside of the
/// solid also becomes an element surface (`*SURFACE, TYPE=ELEMENT`) of the same name.
pub fn read_msh(text: &str) -> Result<Model> {
    read_msh_mesh(text).map(|m| m.model)
}

/// Like [`read_msh`], and also report the volumes with no physical group and the surface
/// groups that are not on the outside of the solid.
pub fn read_msh_mesh(text: &str) -> Result<MshMesh> {
    read(text, false)
}

/// Read a shell mesh. The surface elements are the shells, and a group of surfaces is an
/// element set. A group of curves on the edge of the shells also becomes an element surface.
pub fn read_msh_shell(text: &str) -> Result<MshMesh> {
    read(text, true)
}

fn read(text: &str, shell: bool) -> Result<MshMesh> {
    let mut l = Lines {
        lines: text.lines().collect(),
        pos: 0,
    };
    let mut r = Reader {
        shell,
        ..Reader::default()
    };
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
                self.has_volume |= dim == 3;
                if dim == if self.shell { 2 } else { 3 } {
                    self.volumes.push((tag, !tags.is_empty()));
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
                self.add_to_groups(dim, tag, &ids, &[], None);
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
            let inp = inp_type(ty, self.shell);
            let (what, hint) = if self.shell {
                (" in a shell mesh", "9 (S6)")
            } else {
                ("", "11 (C3D10)")
            };
            if tdim >= if self.shell { 2 } else { 3 } && inp.is_none() {
                return fail(
                    code::MSH_UNSUPPORTED,
                    line,
                    format!("element type {ty} is not accepted{what}, use {hint}"),
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
                // The corners come first: 2 for a line, 3 for a triangle, 4 for a quadrangle.
                let face = (tdim == if self.shell { 1 } else { 2 }).then(|| {
                    let corners = match tdim {
                        1 => 2,
                        _ if count == 3 || count == 6 => 3,
                        _ => 4,
                    };
                    let mut c = nodes[..corners].to_vec();
                    c.sort_unstable();
                    c
                });
                self.add_to_groups(dim, tag, &nodes, &elements, face);
            }
        }
        Ok(())
    }

    fn add_to_groups(
        &mut self,
        dim: u8,
        tag: u32,
        nodes: &[u32],
        elements: &[u32],
        face: Option<Vec<u32>>,
    ) {
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
            self.groups[i].faces.extend(face.clone());
        }
    }

    fn finish(mut self) -> Result<MshMesh> {
        // Each element side, by its sorted corner nodes.
        let mut on: HashMap<Vec<u32>, Vec<(u32, u8)>> = HashMap::new();
        if self.groups.iter().any(|g| !g.faces.is_empty()) {
            for e in &self.model.elements {
                for (number, corners) in sides(&e.element_type) {
                    let mut key: Vec<u32> = corners.iter().map(|&c| e.nodes[c]).collect();
                    key.sort_unstable();
                    on.entry(key).or_default().push((e.id, *number));
                }
            }
        }
        let mut inner_surfaces = Vec::new();
        for g in &self.groups {
            if g.faces.is_empty() {
                continue;
            }
            let faces: Option<Vec<(u32, u8)>> = g
                .faces
                .iter()
                .map(|c| match on.get(c).map(Vec::as_slice) {
                    Some(&[one]) => Some(one),
                    _ => None,
                })
                .collect();
            match faces {
                Some(mut faces) => {
                    faces.sort_unstable();
                    faces.dedup();
                    self.model.element_surfaces.push(ElementSurface {
                        name: g.name.clone(),
                        faces,
                    });
                }
                None => inner_surfaces.push(g.name.clone()),
            }
        }
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
        let grouped = self.entities.values().any(|tags| !tags.is_empty());
        Ok(MshMesh {
            model: self.model,
            unnamed_volumes: self
                .volumes
                .iter()
                .filter(|v| grouped && !v.1)
                .map(|v| v.0)
                .collect(),
            has_volume: self.has_volume,
            inner_surfaces,
        })
    }
}
