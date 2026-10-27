//! The reader of `grid.toml`, the case file of the built-in grid solver (`fh guide grid`), and
//! the result grid and the probe table of each output. `fh run` and the browser solve of the
//! viewer use the same code, so the same case gives the same results.

use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{Cell, FaceSet, Frame, Load, Material, Mesh, Model, Probe, Step};
use fh_fem::prism::{Footprint, Outline};
use toml_edit::{Document, Item, TableLike};

use crate::vtu::{Field, Grid, VTK_HEXAHEDRON, VTK_TETRA};

/// The text of an error about a key that the solver does not have.
const CLASS: &str = "The built-in grid solver solves linear heat conduction in boxes and in \
    prisms of polygons and circles along 1 axis, and with a [line] table the per-length \
    parameters of a transmission line. For other curved or sloped shapes, contact, radiation or \
    temperature-dependent materials, use gmsh and CalculiX (fh guide grid)";

/// True when the case `text` is a transmission line: it has a `[line]` table (`crate::line`).
pub fn is_line(text: &str) -> bool {
    Document::parse(text).is_ok_and(|d| d.contains_key("line"))
}

/// The model of `grid.toml` and its unit system.
pub fn read(text: &str) -> Result<(Model, String), String> {
    let doc = Document::parse(text).map_err(|e| e.message().to_owned())?;
    let top: &dyn TableLike = doc.as_table();
    only(
        top,
        "the top",
        &[
            "units", "grid", "material", "part", "face", "load", "step", "probe",
        ],
    )?;
    let units = string(top, "units", "the top")?;
    if units != "SI-m" && units != "SI-mm" {
        return Err(format!("`units` must be SI-m or SI-mm, not {units}"));
    }
    let grid = table(top, "grid")?;
    only(
        grid,
        "[grid]",
        &["max_cell", "max_growth", "min_cells", "symmetry", "axis"],
    )?;
    let axis = match grid.get("axis") {
        None => 2,
        Some(item) => item
            .as_str()
            .and_then(|a| ["x", "y", "z"].iter().position(|x| *x == a))
            .ok_or("[grid]: `axis` must be x, y or z")?,
    };
    let spacing = Spacing {
        max_cell: number(grid, "max_cell", "[grid]")?,
        max_growth: optional(grid, "max_growth", "[grid]")?.unwrap_or(1.3),
        min_cells: match grid.get("min_cells") {
            None => 2,
            Some(item) => item
                .as_integer()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|&v| v >= 1)
                .ok_or("[grid]: `min_cells` must be a whole number, 1 or more")?,
        },
    };
    let mut symmetry = [false; 3];
    if let Some(item) = grid.get("symmetry") {
        for axis in strings(item, "[grid] symmetry")? {
            let d = ["x", "y", "z"]
                .iter()
                .position(|a| *a == axis)
                .ok_or_else(|| format!("[grid]: symmetry `{axis}` must be x, y or z"))?;
            symmetry[d] = true;
        }
    }
    let mut materials = Vec::new();
    for (name, item) in named(top, "material")? {
        let where_ = format!("[material.{name}]");
        only(item, &where_, &["conductivity", "density", "specific_heat"])?;
        let k = item
            .get("conductivity")
            .ok_or_else(|| format!("{where_}: missing key `conductivity`"))?;
        let conductivity = match numbers(k) {
            Some(v) if v.len() == 3 => [v[0], v[1], v[2]],
            _ => {
                let v = number(item, "conductivity", &where_).map_err(|_| {
                    format!("{where_}: `conductivity` must be a number or 3 numbers (x, y, z)")
                })?;
                [v; 3]
            }
        };
        let density = optional(item, "density", &where_)?;
        let heat = optional(item, "specific_heat", &where_)?;
        let capacity = match (density, heat) {
            (Some(r), Some(c)) => r * c,
            (None, None) => 0.0,
            _ => {
                return Err(format!(
                    "{where_}: give both density and specific_heat, or neither"
                ))
            }
        };
        materials.push(Material {
            name: name.to_owned(),
            conductivity,
            capacity,
        });
    }
    let mut parts = Vec::new();
    for (name, item) in named(top, "part")? {
        let where_ = format!("[part.{name}]");
        only(
            item,
            &where_,
            &[
                "material", "x", "y", "z", "priority", "polygon", "circle", "holes",
            ],
        )?;
        let m = string(item, "material", &where_)?;
        let material = materials
            .iter()
            .position(|x| x.name == m)
            .ok_or_else(|| format!("{where_}: no material {m}"))?;
        let footprint = footprint(item, &where_, axis)?;
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for (d, name) in ["x", "y", "z"].iter().enumerate() {
            if footprint.is_some() && d != axis {
                if item.contains_key(name) {
                    return Err(format!(
                        "{where_}: a part with a polygon or a circle has only the range `{}` \
                         along the axis; its footprint gives {name}",
                        ["x", "y", "z"][axis]
                    ));
                }
                continue;
            }
            let range = item
                .get(name)
                .and_then(numbers)
                .filter(|v| v.len() == 2)
                .ok_or_else(|| {
                    format!("{where_}: `{name}` must be 2 numbers, the range of the part")
                })?;
            lo[d] = range[0];
            hi[d] = range[1];
        }
        let priority = match item.get("priority") {
            None => 0,
            Some(p) => p
                .as_integer()
                .ok_or(format!("{where_}: `priority` must be a whole number"))?,
        };
        parts.push(Part {
            name: name.to_owned(),
            material,
            priority,
            lo,
            hi,
            footprint,
        });
    }
    let part_index = |name: &str, where_: &str| {
        parts
            .iter()
            .position(|p: &Part| p.name == name)
            .ok_or_else(|| format!("{where_}: no part {name}"))
    };
    let mut faces = Vec::new();
    for (name, item) in named(top, "face")? {
        let where_ = format!("[face.{name}]");
        only(item, &where_, &["side", "box", "parts", "circle"])?;
        let sides = match item.get("side") {
            None => Vec::new(),
            Some(s) => strings(s, &where_)?
                .iter()
                .map(|s| {
                    Side::parse(s).ok_or_else(|| {
                        format!("{where_}: side `{s}` must be x-, x+, y-, y+, z- or z+")
                    })
                })
                .collect::<Result<_, _>>()?,
        };
        let region = match item.get("box") {
            None => None,
            Some(b) => {
                let v = numbers(b).filter(|v| v.len() == 6).ok_or_else(|| {
                    format!("{where_}: `box` must be 6 numbers: x0, y0, z0, x1, y1, z1")
                })?;
                Some(([v[0], v[1], v[2]], [v[3], v[4], v[5]]))
            }
        };
        let parts = match item.get("parts") {
            None => Vec::new(),
            Some(p) => strings(p, &where_)?
                .iter()
                .map(|p| part_index(p, &where_))
                .collect::<Result<_, _>>()?,
        };
        let circle = match item.get("circle") {
            None => None,
            Some(c) => {
                let v = numbers(c).filter(|v| v.len() == 3).ok_or_else(|| {
                    format!("{where_}: `circle` must be 3 numbers: the centre and the radius")
                })?;
                Some(([v[0], v[1]], v[2]))
            }
        };
        faces.push(FaceSet {
            name: name.to_owned(),
            sides,
            region,
            parts,
            circle,
        });
    }
    let mut loads = Vec::new();
    if let Some(list) = top.get("load") {
        let list = list
            .as_array_of_tables()
            .ok_or("`load` must be a list of tables: [[load]]")?;
        for (i, item) in list.iter().enumerate() {
            let where_ = format!("[[load]] {}", i + 1);
            let face = |item: &dyn TableLike| -> Result<usize, String> {
                let f = string(item, "face", &where_)?;
                faces
                    .iter()
                    .position(|x: &FaceSet| x.name == f)
                    .ok_or_else(|| format!("{where_}: no face set {f}"))
            };
            let load = if item.contains_key("temperature") {
                only(item, &where_, &["temperature", "face"])?;
                Load::Temperature {
                    faces: face(item)?,
                    value: number(item, "temperature", &where_)?,
                }
            } else if item.contains_key("flux") {
                only(item, &where_, &["flux", "face"])?;
                Load::Flux {
                    faces: face(item)?,
                    value: number(item, "flux", &where_)?,
                }
            } else if item.contains_key("convection") {
                only(item, &where_, &["convection", "sink", "face"])?;
                Load::Convection {
                    faces: face(item)?,
                    film: number(item, "convection", &where_)?,
                    sink: number(item, "sink", &where_)?,
                }
            } else if item.contains_key("source") {
                only(item, &where_, &["source", "part"])?;
                Load::Source {
                    part: part_index(&string(item, "part", &where_)?, &where_)?,
                    value: number(item, "source", &where_)?,
                }
            } else {
                only(item, &where_, &[])?;
                return Err(format!(
                    "{where_}: a load needs temperature, flux, convection or source"
                ));
            };
            loads.push(load);
        }
    }
    let step = table(top, "step")?;
    let step = match string(step, "type", "[step]")?.as_str() {
        "steady" => {
            only(step, "[step]", &["type"])?;
            Step::Steady
        }
        "transient" => {
            only(
                step,
                "[step]",
                &["type", "end", "dt", "initial", "output_every"],
            )?;
            Step::Transient {
                end: number(step, "end", "[step]")?,
                dt: number(step, "dt", "[step]")?,
                initial: number(step, "initial", "[step]")?,
                output_every: match step.get("output_every") {
                    None => 1,
                    Some(item) => item
                        .as_integer()
                        .and_then(|v| usize::try_from(v).ok())
                        .filter(|&v| v >= 1)
                        .ok_or("[step]: `output_every` must be a whole number, 1 or more")?,
                },
            }
        }
        other => {
            return Err(format!(
                "[step]: `type` must be steady or transient, not {other}. {CLASS}"
            ))
        }
    };
    let mut probes = Vec::new();
    for (name, item) in named(top, "probe")? {
        let where_ = format!("[probe.{name}]");
        only(item, &where_, &["point"])?;
        let p = item
            .get("point")
            .and_then(numbers)
            .filter(|v| v.len() == 3)
            .ok_or_else(|| format!("{where_}: `point` must be 3 numbers"))?;
        probes.push(Probe {
            name: name.to_owned(),
            point: [p[0], p[1], p[2]],
        });
    }
    if let Some(p) = probes.iter().find(|p| {
        !p.name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    }) {
        return Err(format!(
            "[probe.{}]: a probe name has only letters, digits, `_`, `-` and `.`",
            p.name
        ));
    }
    Ok((
        Model {
            parts,
            materials,
            spacing,
            axis,
            symmetry,
            faces,
            loads,
            step,
            probes,
            method: None,
        },
        units,
    ))
}

/// The footprint of a part from `polygon` or `circle`, and `holes`, or `None` for a box.
pub(crate) fn footprint(
    item: &dyn TableLike,
    where_: &str,
    axis: usize,
) -> Result<Option<Footprint>, String> {
    let plane = {
        let a = ["x", "y", "z"];
        format!("{} and {}", a[(axis + 1) % 3], a[(axis + 2) % 3])
    };
    let outline = match (item.get("polygon"), item.get("circle")) {
        (None, None) => {
            if item.contains_key("holes") {
                return Err(format!("{where_}: `holes` needs a polygon or a circle"));
            }
            return Ok(None);
        }
        (Some(_), Some(_)) => {
            return Err(format!("{where_}: give a polygon or a circle, not both"));
        }
        (Some(p), None) => polygon_of(p)
            .ok_or_else(|| format!("{where_}: a polygon is 3 or more corners [[{plane}], ...]"))?,
        (None, Some(c)) => circle_of(c).ok_or_else(|| {
            format!("{where_}: a circle is 3 numbers [{plane} of the centre, radius]")
        })?,
    };
    let holes = match item.get("holes") {
        None => Vec::new(),
        Some(h) => h
            .as_array()
            .ok_or_else(|| format!("{where_}: `holes` must be a list of polygons and circles"))?
            .iter()
            .map(|v| {
                let v = Item::Value(v.clone());
                circle_of(&v).or_else(|| polygon_of(&v)).ok_or_else(|| {
                    format!(
                        "{where_}: a hole is a circle of 3 numbers [{plane} of the centre, \
                         radius] or a polygon of 3 or more corners [[{plane}], ...]"
                    )
                })
            })
            .collect::<Result<_, _>>()?,
    };
    Ok(Some(Footprint { outline, holes }))
}

/// A circle `[c1, c2, r]`.
fn circle_of(item: &Item) -> Option<Outline> {
    let v = numbers(item)?;
    (v.len() == 3).then(|| Outline::Circle {
        centre: [v[0], v[1]],
        radius: v[2],
    })
}

/// A polygon `[[a1, a2], [b1, b2], ...]` of 3 or more corners.
fn polygon_of(item: &Item) -> Option<Outline> {
    let corners: Vec<[f64; 2]> = item
        .as_array()?
        .iter()
        .map(|c| {
            let v = numbers(&Item::Value(c.clone()))?;
            (v.len() == 2).then(|| [v[0], v[1]])
        })
        .collect::<Option<_>>()?;
    (corners.len() >= 3).then_some(Outline::Polygon(corners))
}

/// An error for each key of `table` that is not in `keys`.
fn only(table: &dyn TableLike, where_: &str, keys: &[&str]) -> Result<(), String> {
    only_in(table, where_, keys, CLASS)
}

/// An error for each key of `table` that is not in `keys`, with the text `class`.
pub(crate) fn only_in(
    table: &dyn TableLike,
    where_: &str,
    keys: &[&str],
    class: &str,
) -> Result<(), String> {
    match table.iter().find(|(k, _)| !keys.contains(k)) {
        Some((key, _)) => Err(format!("{where_}: unknown key `{key}`. {class}")),
        None => Ok(()),
    }
}

pub(crate) fn table<'a>(top: &'a dyn TableLike, key: &str) -> Result<&'a dyn TableLike, String> {
    top.get(key)
        .ok_or_else(|| format!("missing table [{key}]"))?
        .as_table_like()
        .ok_or_else(|| format!("`{key}` must be a table [{key}]"))
}

/// The tables `[key.NAME]`, in the order of the file.
pub(crate) fn named<'a>(
    top: &'a dyn TableLike,
    key: &str,
) -> Result<Vec<(&'a str, &'a dyn TableLike)>, String> {
    let Some(item) = top.get(key) else {
        return Ok(Vec::new());
    };
    let t = item
        .as_table_like()
        .ok_or_else(|| format!("`{key}` must be tables [{key}.NAME]"))?;
    t.iter()
        .map(|(name, item)| {
            let inner = item
                .as_table_like()
                .ok_or_else(|| format!("`{key}.{name}` must be a table [{key}.{name}]"))?;
            Ok((name, inner))
        })
        .collect()
}

pub(crate) fn string(table: &dyn TableLike, key: &str, where_: &str) -> Result<String, String> {
    table
        .get(key)
        .ok_or_else(|| format!("{where_}: missing key `{key}`"))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{where_}: `{key}` must be a string"))
}

/// A string or a list of strings.
pub(crate) fn strings(item: &Item, where_: &str) -> Result<Vec<String>, String> {
    if let Some(s) = item.as_str() {
        return Ok(vec![s.to_owned()]);
    }
    item.as_array()
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_owned)).collect())
        .ok_or_else(|| format!("{where_}: expected a string or a list of strings"))
}

pub(crate) fn as_number(item: &Item) -> Option<f64> {
    item.as_float()
        .or_else(|| item.as_integer().map(|i| i as f64))
        .filter(|v| v.is_finite())
}

/// A list of finite numbers.
pub(crate) fn numbers(item: &Item) -> Option<Vec<f64>> {
    item.as_array()?
        .iter()
        .map(|v| {
            v.as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .filter(|v| v.is_finite())
        })
        .collect()
}

pub(crate) fn optional(
    table: &dyn TableLike,
    key: &str,
    where_: &str,
) -> Result<Option<f64>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(item) => as_number(item)
            .map(Some)
            .ok_or_else(|| format!("{where_}: `{key}` must be a finite number")),
    }
}

pub(crate) fn number(table: &dyn TableLike, key: &str, where_: &str) -> Result<f64, String> {
    optional(table, key, where_)?.ok_or_else(|| format!("{where_}: missing key `{key}`"))
}

/// The result grid of `frame`: the temperature `NT` at each point, and the heat flux `HFL` and
/// the part number `part` (from 1) of each cell.
pub fn frame_grid(mesh: &Mesh, frame: &Frame) -> Grid {
    let kind = match mesh.kind {
        Cell::Hex8 => VTK_HEXAHEDRON,
        Cell::Tet4 => VTK_TETRA,
    };
    Grid {
        points: mesh.points.clone(),
        cell_types: vec![kind; mesh.len()],
        cells: (0..mesh.len()).map(|e| mesh.cell(e).to_vec()).collect(),
        point_data: vec![Field::f64("NT", 1, frame.temperature.clone())],
        cell_data: vec![
            Field::f64("HFL", 3, frame.flux.iter().flatten().copied().collect()),
            Field::f64(
                "part",
                1,
                mesh.part.iter().map(|&p| f64::from(p) + 1.0).collect(),
            ),
        ],
    }
}

/// The first line of the probe table: `# time` and the name of each probe.
pub fn probe_head(model: &Model) -> String {
    let names: Vec<&str> = model.probes.iter().map(|p| p.name.as_str()).collect();
    format!("# time {}\n", names.join(" "))
}

/// The line of the probe table for `frame`: the time and the temperature at each probe.
pub fn probe_row(frame: &Frame) -> String {
    let row: Vec<String> = std::iter::once(frame.time)
        .chain(frame.probes.iter().copied())
        .map(|v| format!("{v:.10e}"))
        .collect();
    format!("{}\n", row.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fh_fem::heat::{self, Events};

    const WALL: &str = include_str!("../../../validation/cases/grid-layered-wall/wall/grid.toml");
    const PLATE: &str =
        include_str!("../../../validation/cases/grid-plate-hole-ccx/plate/grid.toml");

    #[test]
    fn the_reader_reads_a_case() {
        let (model, units) = read(WALL).unwrap();
        assert_eq!(units, "SI-m");
        assert_eq!(
            (model.parts.len(), model.materials.len(), model.probes.len()),
            (3, 3, 4)
        );
    }

    /// A key out of the class of the solver gives the text that names gmsh and CalculiX, not a
    /// solve that ignores the key.
    #[test]
    fn an_unknown_key_is_refused_with_the_class_of_the_solver() {
        for (text, key) in [
            (WALL.replace("[grid]", "[grid]\ncurved = true"), "curved"),
            (
                WALL.replace("conductivity = 0.5", "conductivity = 0.5\nemissivity = 0.9"),
                "emissivity",
            ),
            (
                WALL.replace("type = \"steady\"", "type = \"static\""),
                "static",
            ),
        ] {
            let e = read(&text).unwrap_err();
            assert!(e.contains(key) && e.contains("CalculiX"), "{e}");
        }
    }

    /// A polygon part with a circle hole, a box part and a face set on the circle.
    #[test]
    fn the_reader_reads_polygon_and_circle_parts() {
        let (model, _) = read(PLATE).unwrap();
        assert_eq!(model.axis, 2);
        let plate = model.parts[0].footprint.as_ref().unwrap();
        assert!(matches!(&plate.outline, Outline::Polygon(c) if c.len() == 4));
        assert_eq!(
            plate.holes,
            vec![Outline::Circle {
                centre: [0.03, 0.03],
                radius: 0.01
            }]
        );
        assert_eq!((model.parts[0].lo[2], model.parts[0].hi[2]), (0.0, 0.01));
        assert!(model.parts[1].footprint.is_none());
        assert_eq!(model.faces[0].circle, Some(([0.03, 0.03], 0.01)));
        let along_x = PLATE.replace("[grid]", "[grid]\naxis = \"x\"").replacen(
            "z = [0.0, 0.01]",
            "x = [0.0, 0.01]",
            1,
        );
        let x = read(&along_x).unwrap().0;
        assert_eq!((x.axis, x.parts[0].lo[0], x.parts[0].hi[0]), (0, 0.0, 0.01));
    }

    /// A prism part with a range across its axis, a bad outline, or symmetry is refused with a
    /// message that tells why.
    #[test]
    fn a_wrong_prism_part_is_refused() {
        let with_x = PLATE.replace("z = [0.0, 0.01]", "z = [0.0, 0.01]\nx = [0.0, 0.1]");
        let e = read(&with_x).unwrap_err();
        assert!(e.contains("only the range `z`"), "{e}");
        let both = PLATE.replace("holes = ", "circle = [0.0, 0.0, 1.0]\nholes = ");
        assert!(read(&both).unwrap_err().contains("not both"));
        let bad = PLATE.replace("[[0.03, 0.03, 0.01]]", "[[0.03, 0.03]]");
        assert!(read(&bad)
            .unwrap_err()
            .contains("a hole is a circle of 3 numbers"));
        let e = read(&PLATE.replace(
            "polygon = [[0.0, 0.0], [0.1, 0.0], [0.1, 0.06], [0.0, 0.06]]",
            "polygon = [0.05, 0.03, 0.04]",
        ))
        .unwrap_err();
        assert!(e.contains("a polygon is 3 or more corners"), "{e}");
        let e = read(&PLATE.replace("polygon = ", "circle = ")).unwrap_err();
        assert!(e.contains("a circle is 3 numbers"), "{e}");
        let sym = PLATE.replace("[grid]", "[grid]\nsymmetry = [\"x\"]");
        let (model, _) = read(&sym).unwrap();
        struct Quiet;
        impl Events for Quiet {
            fn mesh(&mut self, _: &Mesh) -> Result<(), String> {
                Ok(())
            }
            fn progress(&mut self, _: &str) {}
            fn frame(&mut self, _: &Frame) -> Result<(), String> {
                Ok(())
            }
        }
        let e = heat::solve(&model, &mut Quiet).unwrap_err();
        assert!(e.contains("symmetry needs a model of boxes"), "{e}");
    }
}
