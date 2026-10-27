//! The reader of `grid.toml`, the case file of the built-in grid solver (`fh guide grid`), and
//! the result grid and the probe table of each output. `fh run` and the browser solve of the
//! viewer use the same code, so the same case gives the same results.

use fh_fem::elastic::{self, Surface, Thermal};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{Cell, FaceSet, Frame, Load, Material, Mesh, Model, Probe, Step};
use fh_fem::hex::Orthotropic;
use fh_fem::homog;
use fh_fem::prism::{Footprint, Outline};
use toml_edit::{Document, Item, TableLike};

use crate::vtu::{Field, Grid, VTK_HEXAHEDRON, VTK_TETRA};

/// The text of an error about a key that the solver does not have.
const CLASS: &str = "The built-in grid solver solves linear heat conduction in boxes and in \
    prisms of polygons and circles along 1 axis, linear static thermo-elasticity in boxes, the \
    homogenized material of a periodic unit cell, and with a [line] table the per-length parameters of a transmission line. For other curved or \
    sloped shapes, contact, radiation, plasticity, large deformation, vibration or \
    temperature-dependent materials, use gmsh and CalculiX (fh guide grid)";

/// True when the case `text` is a transmission line: it has a `[line]` table (`crate::line`).
pub fn is_line(text: &str) -> bool {
    Document::parse(text).is_ok_and(|d| d.contains_key("line"))
}

/// The model of a grid case.
#[derive(Debug, Clone, PartialEq)]
pub enum Case {
    /// A steady or transient heat conduction.
    Heat(Model),
    /// A linear static stress, with the temperature of a steady heat solve or a uniform one.
    Elastic(elastic::Model),
    /// The homogenized material of a periodic unit cell.
    Homogenize(homog::Model),
}

/// The keys of a material that a static step reads.
#[derive(Default)]
struct Elastic {
    youngs: Option<Vec<f64>>,
    poisson: Option<Vec<f64>>,
    shear: Option<Vec<f64>>,
    expansion: Option<Vec<f64>>,
    density: f64,
}

/// The model of `grid.toml` and its unit system.
pub fn read(text: &str) -> Result<(Case, String), String> {
    let doc = Document::parse(text).map_err(|e| e.message().to_owned())?;
    let top: &dyn TableLike = doc.as_table();
    only(
        top,
        "the top",
        &[
            "units", "grid", "material", "part", "face", "load", "step", "probe", "surface",
        ],
    )?;
    let units = string(top, "units", "the top")?;
    if units != "SI-m" && units != "SI-mm" {
        return Err(format!("`units` must be SI-m or SI-mm, not {units}"));
    }
    let step = table(top, "step")?;
    let kind = string(step, "type", "[step]")?;
    if !["steady", "transient", "static", "homogenize"].contains(&kind.as_str()) {
        return Err(format!(
            "[step]: `type` must be steady, transient, static or homogenize, not {kind}. {CLASS}"
        ));
    }
    let is_static = kind == "static";
    let is_cell = kind == "homogenize";
    if is_cell {
        only(step, "[step]", &["type"])?;
        for key in ["face", "load", "probe", "surface"] {
            if top.contains_key(key) {
                return Err(format!(
                    "a homogenize step has no `{key}`: the unit cell has periodic conditions on \
                     its 6 sides, and the result is its effective material"
                ));
            }
        }
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
    // The heat keys of each material, with a missing conductivity, and the elastic keys.
    let mut materials = Vec::new();
    let mut conductive = Vec::new();
    let mut elastic = Vec::new();
    let mut cell_materials = Vec::new();
    for (name, item) in named(top, "material")? {
        let where_ = format!("[material.{name}]");
        only(
            item,
            &where_,
            &[
                "conductivity",
                "density",
                "specific_heat",
                "youngs_modulus",
                "poisson_ratio",
                "shear_modulus",
                "expansion",
            ],
        )?;
        let conductivity = match item.get("conductivity") {
            None => None,
            Some(k) => Some(match numbers(k) {
                Some(v) if v.len() == 3 => [v[0], v[1], v[2]],
                _ => {
                    let v = number(item, "conductivity", &where_).map_err(|_| {
                        format!("{where_}: `conductivity` must be a number or 3 numbers (x, y, z)")
                    })?;
                    [v; 3]
                }
            }),
        };
        let density = optional(item, "density", &where_)?;
        let heat = optional(item, "specific_heat", &where_)?;
        let capacity = match (density, heat) {
            (Some(r), Some(c)) => r * c,
            (None, None) => 0.0,
            // A static step uses the density alone, for gravity, and a unit cell for its mean.
            (Some(_), None) if is_static || is_cell => 0.0,
            _ => {
                return Err(format!(
                    "{where_}: give both density and specific_heat, or neither"
                ))
            }
        };
        if is_cell {
            cell_materials.push(homog::Material {
                name: name.to_owned(),
                conductivity,
                elastic: None,
                expansion: [0.0; 3],
                density,
                specific_heat: heat,
            });
        }
        conductive.push(conductivity.is_some());
        materials.push(Material {
            name: name.to_owned(),
            conductivity: conductivity.unwrap_or([0.0; 3]),
            capacity,
        });
        elastic.push(Elastic {
            youngs: one_or_three(item, "youngs_modulus", &where_)?,
            poisson: one_or_three(item, "poisson_ratio", &where_)?,
            shear: one_or_three(item, "shear_modulus", &where_)?,
            expansion: one_or_three(item, "expansion", &where_)?,
            density: density.unwrap_or(0.0),
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
    let face_index = |item: &dyn TableLike, where_: &str| -> Result<usize, String> {
        let f = string(item, "face", where_)?;
        faces
            .iter()
            .position(|x: &FaceSet| x.name == f)
            .ok_or_else(|| format!("{where_}: no face set {f}"))
    };
    let vector = |item: &dyn TableLike, key: &str, where_: &str| -> Result<[f64; 3], String> {
        item.get(key)
            .and_then(numbers)
            .filter(|v| v.len() == 3)
            .map(|v| [v[0], v[1], v[2]])
            .ok_or_else(|| format!("{where_}: `{key}` must be 3 numbers (x, y, z)"))
    };
    let mut loads = Vec::new();
    let mut forces = Vec::new();
    if let Some(list) = top.get("load") {
        let list = list
            .as_array_of_tables()
            .ok_or("`load` must be a list of tables: [[load]]")?;
        for (i, item) in list.iter().enumerate() {
            let where_ = format!("[[load]] {}", i + 1);
            let elastic_key = [
                "ux",
                "uy",
                "uz",
                "pressure",
                "traction",
                "gravity",
                "body_force",
            ]
            .into_iter()
            .find(|k| item.contains_key(k));
            if let Some(key) = elastic_key.filter(|_| !is_static) {
                return Err(format!(
                    "{where_}: `{key}` is a load of a static step: [step] type = \"static\""
                ));
            }
            if item.contains_key("temperature") {
                only(item, &where_, &["temperature", "face"])?;
                loads.push(Load::Temperature {
                    faces: face_index(item, &where_)?,
                    value: number(item, "temperature", &where_)?,
                });
            } else if item.contains_key("flux") {
                only(item, &where_, &["flux", "face"])?;
                loads.push(Load::Flux {
                    faces: face_index(item, &where_)?,
                    value: number(item, "flux", &where_)?,
                });
            } else if item.contains_key("convection") {
                only(item, &where_, &["convection", "sink", "face"])?;
                loads.push(Load::Convection {
                    faces: face_index(item, &where_)?,
                    film: number(item, "convection", &where_)?,
                    sink: number(item, "sink", &where_)?,
                });
            } else if item.contains_key("source") {
                only(item, &where_, &["source", "part"])?;
                loads.push(Load::Source {
                    part: part_index(&string(item, "part", &where_)?, &where_)?,
                    value: number(item, "source", &where_)?,
                });
            } else if ["ux", "uy", "uz"].iter().any(|k| item.contains_key(k)) {
                let value = [
                    optional(item, "ux", &where_)?,
                    optional(item, "uy", &where_)?,
                    optional(item, "uz", &where_)?,
                ];
                if item.contains_key("point") {
                    only(item, &where_, &["ux", "uy", "uz", "point"])?;
                    forces.push(elastic::Load::Point {
                        point: vector(item, "point", &where_)?,
                        value,
                    });
                } else {
                    only(item, &where_, &["ux", "uy", "uz", "face"])?;
                    forces.push(elastic::Load::Displacement {
                        faces: face_index(item, &where_)?,
                        value,
                    });
                }
            } else if item.contains_key("pressure") {
                only(item, &where_, &["pressure", "face"])?;
                forces.push(elastic::Load::Pressure {
                    faces: face_index(item, &where_)?,
                    value: number(item, "pressure", &where_)?,
                });
            } else if item.contains_key("traction") {
                only(item, &where_, &["traction", "face"])?;
                forces.push(elastic::Load::Traction {
                    faces: face_index(item, &where_)?,
                    value: vector(item, "traction", &where_)?,
                });
            } else if item.contains_key("gravity") {
                only(item, &where_, &["gravity"])?;
                forces.push(elastic::Load::Gravity(vector(item, "gravity", &where_)?));
            } else if item.contains_key("body_force") {
                only(item, &where_, &["body_force", "part"])?;
                forces.push(elastic::Load::BodyForce {
                    part: part_index(&string(item, "part", &where_)?, &where_)?,
                    value: vector(item, "body_force", &where_)?,
                });
            } else {
                only(item, &where_, &[])?;
                return Err(format!(
                    "{where_}: a load needs temperature, flux, convection or source{}",
                    if is_static {
                        ", or ux, uy, uz, pressure, traction, gravity or body_force"
                    } else {
                        ""
                    }
                ));
            }
        }
    }
    let probes = names(
        named(top, "probe")?
            .into_iter()
            .map(|(name, item)| {
                let where_ = format!("[probe.{name}]");
                only(item, &where_, &["point"])?;
                let p = item
                    .get("point")
                    .and_then(numbers)
                    .filter(|v| v.len() == 3)
                    .ok_or_else(|| format!("{where_}: `point` must be 3 numbers"))?;
                Ok(Probe {
                    name: name.to_owned(),
                    point: [p[0], p[1], p[2]],
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        "probe",
        |p| &p.name,
    )?;
    let surfaces = names(
        named(top, "surface")?
            .into_iter()
            .map(|(name, item)| {
                let where_ = format!("[surface.{name}]");
                if !is_static {
                    return Err(format!(
                        "{where_}: a surface is a result of a static step: [step] type = \"static\""
                    ));
                }
                only(item, &where_, &["face"])?;
                Ok(Surface {
                    name: name.to_owned(),
                    faces: face_index(item, &where_)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        "surface",
        |s| &s.name,
    )?;
    if is_cell {
        if symmetry.iter().any(|&s| s) {
            return Err(
                "[grid]: a homogenize step has no `symmetry`: give the whole unit cell".into(),
            );
        }
        for (m, e) in cell_materials.iter_mut().zip(&elastic) {
            let where_ = format!("[material.{}]", m.name);
            if e.youngs.is_some() || e.poisson.is_some() || e.shear.is_some() {
                m.elastic = Some(orthotropic(e, &where_)?);
            }
            m.expansion = expansion(e, &where_)?;
        }
        return Ok((
            Case::Homogenize(homog::Model {
                parts,
                materials: cell_materials,
                spacing,
                axis,
            }),
            units,
        ));
    }
    let missing = |what: &str| {
        conductive
            .iter()
            .zip(&materials)
            .find(|(c, _)| !**c)
            .map(|(_, m)| format!("[material.{}]: missing key `conductivity`{what}", m.name))
    };
    let heat_step = match kind.as_str() {
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
        "steady" => {
            only(step, "[step]", &["type"])?;
            Step::Steady
        }
        _ => {
            only(
                step,
                "[step]",
                &["type", "reference_temperature", "temperature"],
            )?;
            Step::Steady
        }
    };
    let heat_model = |probes: Vec<Probe>| Model {
        parts: parts.clone(),
        materials: materials.clone(),
        spacing,
        axis,
        symmetry,
        faces: faces.clone(),
        loads: loads.clone(),
        step: heat_step,
        probes,
        method: None,
    };
    if !is_static {
        if let Some(e) = missing("") {
            return Err(e);
        }
        return Ok((Case::Heat(heat_model(probes)), units));
    }
    let uniform = optional(step, "temperature", "[step]")?;
    let thermal = match (uniform, loads.is_empty()) {
        (Some(_), false) => {
            return Err(
                "[step]: give `temperature` or heat loads (temperature, flux, convection, \
                 source), not both"
                    .into(),
            )
        }
        (Some(t), true) => Thermal::Uniform(t),
        (None, false) => {
            if let Some(e) = missing(", for the heat loads") {
                return Err(e);
            }
            Thermal::Heat(Box::new(heat_model(Vec::new())))
        }
        (None, true) => Thermal::None,
    };
    let reference = optional(step, "reference_temperature", "[step]")?;
    let reference = match (&thermal, reference) {
        (Thermal::None, None) => 0.0,
        (Thermal::None, Some(_)) => {
            return Err("[step]: `reference_temperature` needs `temperature` or heat loads".into())
        }
        (_, Some(r)) => r,
        (_, None) => {
            return Err(
                "[step]: missing key `reference_temperature`, the temperature with no thermal \
                 strain"
                    .into(),
            )
        }
    };
    let materials = materials
        .iter()
        .zip(elastic)
        .map(|(m, e)| {
            let where_ = format!("[material.{}]", m.name);
            Ok(elastic::Material {
                name: m.name.clone(),
                elastic: orthotropic(&e, &where_)?,
                expansion: expansion(&e, &where_)?,
                density: e.density,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok((
        Case::Elastic(elastic::Model {
            parts,
            materials,
            spacing,
            symmetry,
            faces,
            loads: forces,
            thermal,
            reference,
            probes,
            surfaces,
            method: None,
        }),
        units,
    ))
}

/// `items`, with an error for a name with other characters than letters, digits, `_`, `-` and
/// `.`: the name is the start of a column name.
fn names<T>(items: Vec<T>, key: &str, name: impl Fn(&T) -> &String) -> Result<Vec<T>, String> {
    match items.iter().map(&name).find(|n| {
        !n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    }) {
        Some(n) => Err(format!(
            "[{key}.{n}]: a {key} name has only letters, digits, `_`, `-` and `.`"
        )),
        None => Ok(items),
    }
}

/// A number or a list of 3 numbers.
fn one_or_three(
    table: &dyn TableLike,
    key: &str,
    where_: &str,
) -> Result<Option<Vec<f64>>, String> {
    let Some(item) = table.get(key) else {
        return Ok(None);
    };
    as_number(item)
        .map(|v| vec![v])
        .or_else(|| numbers(item).filter(|v| v.len() == 3))
        .map(Some)
        .ok_or_else(|| format!("{where_}: `{key}` must be a number or 3 numbers"))
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

/// The elastic constants of a material: isotropic or orthotropic.
fn orthotropic(e: &Elastic, where_: &str) -> Result<Orthotropic, String> {
    match (&e.youngs, &e.poisson, &e.shear) {
        (Some(y), Some(p), None) if y.len() == 1 && p.len() == 1 => {
            Ok(Orthotropic::isotropic(y[0], p[0]))
        }
        (Some(y), Some(p), Some(g)) if y.len() == 3 && p.len() == 3 && g.len() == 3 => {
            Ok(Orthotropic {
                e: [y[0], y[1], y[2]],
                nu: [p[0], p[1], p[2]],
                g: [g[0], g[1], g[2]],
            })
        }
        _ => Err(format!(
            "{where_}: give youngs_modulus and poisson_ratio as 1 number each (isotropic), or \
             youngs_modulus (x, y, z), poisson_ratio (xy, xz, yz) and shear_modulus (xy, xz, yz) \
             as 3 numbers each (orthotropic)"
        )),
    }
}

/// The expansion coefficient of a material along x, y and z, 0 when it has none.
fn expansion(e: &Elastic, where_: &str) -> Result<[f64; 3], String> {
    match e.expansion.as_deref() {
        None => Ok([0.0; 3]),
        Some(&[a]) => Ok([a; 3]),
        Some(&[a, b, c]) => Ok([a, b, c]),
        _ => Err(format!("{where_}: missing key `expansion`")),
    }
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
    const BLOCK: &str =
        include_str!("../../../validation/cases/grid-constrained-block/block/grid.toml");
    const HEATED: &str =
        include_str!("../../../validation/cases/grid-heated-block-stress-ccx/block/grid.toml");

    fn heat_case(text: &str) -> Model {
        match read(text) {
            Ok((Case::Heat(m), _)) => m,
            other => panic!("{other:?}"),
        }
    }

    fn elastic(text: &str) -> elastic::Model {
        match read(text) {
            Ok((Case::Elastic(m), _)) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_reader_reads_a_static_step() {
        let m = elastic(BLOCK);
        assert_eq!(m.materials[0].elastic.e, [100e3, 50e3, 20e3]);
        assert_eq!(m.materials[0].expansion, [5e-6, 10e-6, 20e-6]);
        assert_eq!(
            (m.thermal.clone(), m.reference),
            (Thermal::Uniform(120.0), 20.0)
        );
        assert_eq!(m.loads.len(), 3);
        // The heat loads of a static step give the temperature of a steady heat solve.
        let m = elastic(HEATED);
        let Thermal::Heat(heat) = &m.thermal else {
            panic!("{:?}", m.thermal)
        };
        assert_eq!((heat.loads.len(), heat.step.clone()), (2, Step::Steady));
        assert_eq!(heat.materials[1].conductivity, [400.0; 3]);
        assert!(matches!(m.loads[..], [elastic::Load::Point { .. }]));
        assert_eq!(m.surfaces.len(), 1);
    }

    /// The reader reads the parts and materials of a homogenize step.
    #[test]
    fn the_reader_reads_a_unit_cell() {
        let cell = r#"
units = "SI-mm"
[grid]
max_cell = 0.5
[material.a]
conductivity = 2.0
youngs_modulus = 70e3
poisson_ratio = 0.3
expansion = 23e-6
density = 2.7e-9
[material.b]
conductivity = [1.0, 1.0, 3.0]
youngs_modulus = 3e3
poisson_ratio = 0.4
[part.a]
material = "a"
x = [0.0, 1.0]
y = [0.0, 1.0]
z = [0.0, 0.5]
[part.b]
material = "b"
x = [0.0, 1.0]
y = [0.0, 1.0]
z = [0.5, 1.0]
[step]
type = "homogenize"
"#;
        let Ok((Case::Homogenize(m), _)) = read(cell) else {
            panic!("a unit cell");
        };
        assert_eq!(m.materials[0].expansion, [23e-6; 3]);
        assert_eq!(m.materials[0].density, Some(2.7e-9));
        assert_eq!(m.materials[1].conductivity, Some([1.0, 1.0, 3.0]));
        assert!(m.materials[1].elastic.is_some());
        for (text, key) in [
            (cell.replace("[step]", "[face.all]\n\n[step]"), "no `face`"),
            (
                cell.replace("max_cell = 0.5", "max_cell = 0.5\nsymmetry = [\"x\"]"),
                "symmetry",
            ),
            (
                cell.replace("type = \"homogenize\"", "type = \"homogenize\"\nend = 1.0"),
                "end",
            ),
        ] {
            let e = read(&text).unwrap_err();
            assert!(e.contains(key), "{key}: {e}");
        }
    }

    /// Each wrong input of a static step gives an error that names the key.
    #[test]
    fn the_reader_refuses_a_wrong_static_step() {
        for (text, key) in [
            (
                BLOCK.replace("temperature = 120.0\n", ""),
                "needs `temperature` or heat loads",
            ),
            (
                BLOCK.replace("reference_temperature = 20.0\n", ""),
                "reference_temperature",
            ),
            (
                BLOCK.replace("shear_modulus = [20e3, 10e3, 8e3]\n", ""),
                "orthotropic",
            ),
            (
                HEATED.replace("reference_temperature = 20.0", "temperature = 50.0"),
                "not both",
            ),
            (HEATED.replace("conductivity = 400.0\n", ""), "conductivity"),
            (
                WALL.replace("[step]", "[[load]]\ngravity = [0, 0, -9.81]\n\n[step]"),
                "gravity",
            ),
            (
                WALL.replace("[step]", "[surface.s]\nface = \"in\"\n\n[step]"),
                "static",
            ),
        ] {
            let e = read(&text).unwrap_err();
            assert!(e.contains(key), "{key}: {e}");
        }
    }
    const PLATE: &str =
        include_str!("../../../validation/cases/grid-plate-hole-ccx/plate/grid.toml");

    #[test]
    fn the_reader_reads_a_case() {
        let (Case::Heat(model), units) = read(WALL).unwrap() else {
            panic!("a heat case")
        };
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
                WALL.replace("type = \"steady\"", "type = \"frequency\""),
                "frequency",
            ),
        ] {
            let e = read(&text).unwrap_err();
            assert!(e.contains(key) && e.contains("CalculiX"), "{e}");
        }
    }

    /// A polygon part with a circle hole, a box part and a face set on the circle.
    #[test]
    fn the_reader_reads_polygon_and_circle_parts() {
        let model = heat_case(PLATE);
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
        let x = heat_case(&along_x);
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
        let model = heat_case(&sym);
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
