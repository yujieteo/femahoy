//! Build the model of the preview from keyword blocks. A keyword that the preview does not
//! draw is skipped. A data line that does not parse is skipped too. CalculiX checks the rest.

use crate::lexer::{lex, Block};
use crate::model::*;

/// Read the mesh, the sets, the surfaces, the supports and the loads of a CalculiX input.
/// The text must hold the files of `*INCLUDE` already (see [`crate::flatten`]).
pub fn parse(text: &str) -> Model {
    let mut p = Reader::default();
    for block in &lex(text) {
        p.block(block);
    }
    p.end_step();
    p.model
}

#[derive(Default)]
struct Reader {
    model: Model,
    step: Option<Step>,
}

fn id(f: Option<&String>) -> Option<u32> {
    f?.parse::<u32>().ok().filter(|&n| n > 0)
}

/// A finite number. The Rust parser also accepts NaN and infinity.
fn real(f: Option<&String>) -> Option<f64> {
    f?.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn dof(f: Option<&String>) -> Option<u8> {
    f?.parse::<u8>().ok().filter(|&d| d > 0)
}

/// The number after `letter` in `P1` or `S2`, or 0 when there is none.
fn face_number(text: &str, letter: char) -> Option<u8> {
    let n = text.to_uppercase().strip_prefix(letter)?.to_owned();
    if n.is_empty() {
        Some(0)
    } else {
        n.parse().ok()
    }
}

impl Reader {
    fn block(&mut self, b: &Block) {
        match b.keyword.as_str() {
            "NODE" => self.nodes(b),
            "ELEMENT" => self.elements(b),
            "NSET" => self.set(b, true),
            "ELSET" => self.set(b, false),
            "SURFACE" => self.surface(b),
            "STEP" => {
                self.end_step();
                self.step = Some(Step {
                    name: b.value("NAME").map(str::to_owned),
                    ..Step::default()
                });
            }
            "END STEP" => self.end_step(),
            "BOUNDARY" => self.boundary(b),
            "CLOAD" => self.cload(b),
            "DLOAD" => self.dload(b),
            "DSLOAD" => self.dsload(b),
            _ => {}
        }
    }

    fn end_step(&mut self) {
        if let Some(step) = self.step.take() {
            self.model.steps.push(step);
        }
    }

    fn nodes(&mut self, b: &Block) {
        let mut ids = Vec::new();
        for d in &b.data {
            let Some(node) = id(d.first()) else { continue };
            let mut xyz = [0.0; 3];
            for (k, c) in xyz.iter_mut().enumerate().take(d.len().min(4) - 1) {
                *c = real(d.get(k + 1)).unwrap_or(f64::NAN);
            }
            if xyz.iter().any(|c| c.is_nan()) {
                continue;
            }
            self.model.nodes.push(Node { id: node, xyz });
            ids.push(node);
        }
        if let Some(name) = b.value("NSET") {
            add(&mut self.model.nsets, name, ids);
        }
    }

    fn elements(&mut self, b: &Block) {
        let Some(ty) = b.value("TYPE") else { return };
        let mut ids = Vec::new();
        for d in &b.data {
            let Some(element) = id(d.first()) else {
                continue;
            };
            let nodes: Vec<u32> = d[1..].iter().filter_map(|f| f.parse().ok()).collect();
            self.model.elements.push(Element {
                id: element,
                element_type: ty.to_owned(),
                nodes,
            });
            ids.push(element);
        }
        if let Some(name) = b.value("ELSET") {
            add(&mut self.model.elsets, name, ids);
        }
    }

    /// `*NSET` or `*ELSET`: ids, names of earlier sets, or `first, last, step` with `GENERATE`.
    fn set(&mut self, b: &Block, nodes: bool) {
        let Some(name) = b.value(if nodes { "NSET" } else { "ELSET" }) else {
            return;
        };
        let mut out = Vec::new();
        for d in &b.data {
            if b.has("GENERATE") {
                let (Some(first), Some(last)) = (id(d.first()), id(d.get(1))) else {
                    continue;
                };
                let step = id(d.get(2)).unwrap_or(1) as usize;
                out.extend((first..=last).step_by(step));
                continue;
            }
            for f in d.iter().filter(|f| !f.is_empty()) {
                if let Ok(n) = f.parse::<u32>() {
                    out.push(n);
                    continue;
                }
                let known = if nodes {
                    self.model.nset(f).map(|s| &s.ids)
                } else {
                    self.model.elset(f).map(|s| &s.ids)
                };
                out.extend(known.into_iter().flatten());
            }
        }
        if nodes {
            add(&mut self.model.nsets, name, out);
        } else {
            add(&mut self.model.elsets, name, out);
        }
    }

    /// `*SURFACE` of elements. The data lines are `element or element set, S<face>`.
    fn surface(&mut self, b: &Block) {
        if b.value("TYPE").is_some_and(|t| t != "ELEMENT") {
            return;
        }
        let Some(name) = b.value("NAME") else { return };
        let mut faces = Vec::new();
        for d in &b.data {
            let (Some(first), Some(second)) = (d.first(), d.get(1)) else {
                continue;
            };
            let face = match second.to_uppercase().as_str() {
                "SPOS" => 1,
                "SNEG" => 2,
                other => match face_number(other, 'S') {
                    Some(n) if n > 0 => n,
                    _ => continue,
                },
            };
            match first.parse::<u32>() {
                Ok(e) => faces.push((e, face)),
                Err(_) => {
                    let ids = self.model.elset(first).map(|s| s.ids.clone());
                    faces.extend(ids.into_iter().flatten().map(|e| (e, face)));
                }
            }
        }
        self.model.element_surfaces.push(ElementSurface {
            name: name.to_owned(),
            faces,
        });
    }

    fn target(f: &str) -> Target {
        match f.parse::<u32>() {
            Ok(n) => Target::Node(n),
            Err(_) => Target::Set(f.to_uppercase()),
        }
    }

    fn boundary(&mut self, b: &Block) {
        for d in &b.data {
            let (Some(first), Some(first_dof)) = (d.first(), dof(d.get(1))) else {
                continue;
            };
            let last_dof = dof(d.get(2)).unwrap_or(first_dof);
            let boundary = Boundary {
                target: Self::target(first),
                first_dof,
                last_dof: last_dof.max(first_dof),
                value: real(d.get(3)).unwrap_or(0.0),
            };
            match self.step.as_mut() {
                Some(step) => step.boundaries.push(boundary),
                None => self.model.boundaries.push(boundary),
            }
        }
    }

    fn cload(&mut self, b: &Block) {
        let Some(step) = self.step.as_mut() else {
            return;
        };
        for d in &b.data {
            let (Some(first), Some(dof), Some(magnitude)) =
                (d.first(), dof(d.get(1)), real(d.get(2)))
            else {
                continue;
            };
            step.cloads.push(Cload {
                target: Self::target(first),
                dof,
                magnitude,
            });
        }
    }

    fn dload(&mut self, b: &Block) {
        let Some(step) = self.step.as_mut() else {
            return;
        };
        for d in &b.data {
            let (Some(first), Some(face), Some(pressure)) = (
                d.first(),
                d.get(1).and_then(|t| face_number(t, 'P')),
                real(d.get(2)),
            ) else {
                continue;
            };
            let target = match first.parse::<u32>() {
                Ok(e) => ElementTarget::Element(e),
                Err(_) => ElementTarget::Set(first.to_uppercase()),
            };
            step.dloads.push(Dload {
                target,
                face,
                pressure,
            });
        }
    }

    fn dsload(&mut self, b: &Block) {
        let Some(step) = self.step.as_mut() else {
            return;
        };
        for d in &b.data {
            let (Some(surface), Some(pressure)) = (d.first(), real(d.get(2))) else {
                continue;
            };
            if d.get(1).is_some_and(|t| t.eq_ignore_ascii_case("P")) {
                step.dsloads.push(Dsload {
                    surface: surface.to_uppercase(),
                    pressure,
                });
            }
        }
    }
}

/// Add the ids to the set `name`, or make the set.
fn add(sets: &mut Vec<IdSet>, name: &str, ids: Vec<u32>) {
    match sets.iter_mut().find(|s| s.name == name) {
        Some(s) => s.ids.extend(ids),
        None => sets.push(IdSet {
            name: name.to_owned(),
            ids,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESH: &str = "*NODE\n1,0,0,0\n2,1,0,0\n3,0,1,0\n4,0,0,1\n5,.5,0,0\n6,.5,.5,0\n\
7,0,.5,0\n8,0,0,.5\n9,.5,0,.5\n10,0,.5,.5\n\
*ELEMENT, TYPE=C3D10, ELSET=E\n1,1,2,3,4,5,6,7,8,9,10\n";

    #[test]
    fn nodes_elements_and_sets() {
        let m = parse(&format!(
            "{MESH}*NSET, NSET=a\n1, 2\n*NSET, NSET=b\n3\n*NSET, NSET=ab\na, b, 4\n\
*NSET, NSET=g, GENERATE\n5, 9, 2\n"
        ));
        assert_eq!(m.nodes.len(), 10);
        assert_eq!(m.elements[0].element_type, "C3D10");
        assert_eq!(m.elements[0].nodes.len(), 10);
        assert_eq!(m.elset("e").unwrap().ids, vec![1]);
        assert_eq!(m.nset("AB").unwrap().ids, vec![1, 2, 3, 4]);
        assert_eq!(m.nset("g").unwrap().ids, vec![5, 7, 9]);
    }

    #[test]
    fn any_element_type_and_any_other_keyword_pass() {
        let m = parse(
            "*NODE\n1,0,0,0\n*MATERIAL, NAME=M\n*PLASTIC\n250, 0\n*ELEMENT, TYPE=S8R\n1,1,2\n\
*CONTACT PAIR, INTERACTION=I\nA, B\n*STEP\n*FREQUENCY\n5\n*NODE FILE\nU\n*END STEP\n",
        );
        assert_eq!(m.elements[0].element_type, "S8R");
        assert_eq!(m.steps.len(), 1);
    }

    #[test]
    fn supports_and_loads_are_read_by_step() {
        let m = parse(&format!(
            "{MESH}*NSET, NSET=fixed\n1,2,3\n*ELSET, ELSET=top\n1\n\
*SURFACE, TYPE=ELEMENT, NAME=s\ntop, S3\n*BOUNDARY\nfixed, 1, 3\n\
*STEP, NAME=load\n*STATIC\n*CLOAD\n4, 3, -50.\n*DLOAD\ntop, P2, 1.5\n*DSLOAD\ns, P, 2\n\
*BOUNDARY\n4, 2, 2, 0.1\n*END STEP\n"
        ));
        assert_eq!(m.boundaries.len(), 1);
        assert_eq!(m.boundaries[0].target, Target::Set("FIXED".into()));
        assert_eq!(
            (m.boundaries[0].first_dof, m.boundaries[0].last_dof),
            (1, 3)
        );
        assert_eq!(m.element_surface("S").unwrap().faces, vec![(1, 3)]);
        let step = &m.steps[0];
        assert_eq!(step.name.as_deref(), Some("LOAD"));
        assert_eq!(step.cloads[0].target, Target::Node(4));
        assert_eq!(step.cloads[0].magnitude, -50.0);
        assert_eq!(step.dloads[0].target, ElementTarget::Set("TOP".into()));
        assert_eq!(step.dloads[0].face, 2);
        assert_eq!(step.dsloads[0].surface, "S");
        assert_eq!(step.boundaries[0].value, 0.1);
    }

    #[test]
    fn a_shell_load_has_no_face_number() {
        let m = parse("*STEP\n*DLOAD\n1, P, 3\n*END STEP\n");
        assert_eq!(m.steps[0].dloads[0].face, 0);
    }

    #[test]
    fn a_step_without_end_step_is_kept() {
        let m = parse("*STEP\n*CLOAD\n1, 1, 5\n");
        assert_eq!(m.steps[0].cloads.len(), 1);
    }

    #[test]
    fn a_line_that_does_not_parse_is_skipped() {
        let m = parse("*NODE\n1,0,0,0\nx,1,1,1\n2,NaN,0,0\n3,inf,0,0\n4,1e999,0,0\n5,0,0,1\n");
        let ids: Vec<u32> = m.nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![1, 5]);
    }
}
