//! The reader of `grid.toml`, the case file of the built-in grid solver (`fh guide grid`), and
//! the result grid and the probe table of each output. `fh run` and the browser solve of the
//! viewer use the same code, so the same case gives the same results.

use fh_fem::elastic::{self, Surface, Thermal};
use fh_fem::grid::{Part, Side, Spacing};
use fh_fem::heat::{FaceSet, Frame, Load, Material, Mesh, Model, Probe, Step};
use fh_fem::hex::Orthotropic;
use toml_edit::{Document, Item, TableLike};

use crate::vtu::{Field, Grid, VTK_HEXAHEDRON};

/// The text of an error about a key that the solver does not have.
const CLASS: &str = "The built-in grid solver solves linear heat conduction and linear static \
    thermo-elasticity on boxes. For curved or sloped shapes, contact, radiation, plasticity, \
    large deformation, vibration or temperature-dependent materials, use gmsh and CalculiX (fh \
    guide grid)";

/// The model of a grid case.
#[derive(Debug, Clone, PartialEq)]
pub enum Case {
    /// A steady or transient heat conduction.
    Heat(Model),
    /// A linear static stress, with the temperature of a steady heat solve or a uniform one.
    Elastic(elastic::Model),
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
    if !["steady", "transient", "static"].contains(&kind.as_str()) {
        return Err(format!(
            "[step]: `type` must be steady, transient or static, not {kind}. {CLASS}"
        ));
    }
    let is_static = kind == "static";
    let grid = table(top, "grid")?;
    only(
        grid,
        "[grid]",
        &["max_cell", "max_growth", "min_cells", "symmetry"],
    )?;
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
            // A static step uses the density alone, for gravity.
            (Some(_), None) if is_static => 0.0,
            _ => {
                return Err(format!(
                    "{where_}: give both density and specific_heat, or neither"
                ))
            }
        };
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
        only(item, &where_, &["material", "x", "y", "z", "priority"])?;
        let m = string(item, "material", &where_)?;
        let material = materials
            .iter()
            .position(|x| x.name == m)
            .ok_or_else(|| format!("{where_}: no material {m}"))?;
        let mut lo = [0.0; 3];
        let mut hi = [0.0; 3];
        for (d, axis) in ["x", "y", "z"].iter().enumerate() {
            let range = item
                .get(axis)
                .and_then(numbers)
                .filter(|v| v.len() == 2)
                .ok_or_else(|| {
                    format!("{where_}: `{axis}` must be 2 numbers, the range of the box")
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
        only(item, &where_, &["side", "box", "parts"])?;
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
        faces.push(FaceSet {
            name: name.to_owned(),
            sides,
            region,
            parts,
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
            let three = |v: &Option<Vec<f64>>, key: &str| -> Result<[f64; 3], String> {
                match v.as_deref() {
                    Some(&[a]) => Ok([a; 3]),
                    Some(&[a, b, c]) => Ok([a, b, c]),
                    _ => Err(format!("{where_}: missing key `{key}`")),
                }
            };
            let elastic = match (&e.youngs, &e.poisson, &e.shear) {
                (Some(y), Some(p), None) if y.len() == 1 && p.len() == 1 => {
                    Orthotropic::isotropic(y[0], p[0])
                }
                (Some(y), Some(p), Some(g)) if y.len() == 3 && p.len() == 3 && g.len() == 3 => {
                    Orthotropic {
                        e: [y[0], y[1], y[2]],
                        nu: [p[0], p[1], p[2]],
                        g: [g[0], g[1], g[2]],
                    }
                }
                _ => {
                    return Err(format!(
                        "{where_}: give youngs_modulus and poisson_ratio as 1 number each \
                         (isotropic), or youngs_modulus (x, y, z), poisson_ratio (xy, xz, yz) \
                         and shear_modulus (xy, xz, yz) as 3 numbers each (orthotropic)"
                    ))
                }
            };
            Ok(elastic::Material {
                name: m.name.clone(),
                elastic,
                expansion: match e.expansion {
                    None => [0.0; 3],
                    Some(_) => three(&e.expansion, "expansion")?,
                },
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

/// An error for each key of `table` that is not in `keys`.
fn only(table: &dyn TableLike, where_: &str, keys: &[&str]) -> Result<(), String> {
    match table.iter().find(|(k, _)| !keys.contains(k)) {
        Some((key, _)) => Err(format!("{where_}: unknown key `{key}`. {CLASS}")),
        None => Ok(()),
    }
}

fn table<'a>(top: &'a dyn TableLike, key: &str) -> Result<&'a dyn TableLike, String> {
    top.get(key)
        .ok_or_else(|| format!("missing table [{key}]"))?
        .as_table_like()
        .ok_or_else(|| format!("`{key}` must be a table [{key}]"))
}

/// The tables `[key.NAME]`, in the order of the file.
fn named<'a>(
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

fn string(table: &dyn TableLike, key: &str, where_: &str) -> Result<String, String> {
    table
        .get(key)
        .ok_or_else(|| format!("{where_}: missing key `{key}`"))?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{where_}: `{key}` must be a string"))
}

/// A string or a list of strings.
fn strings(item: &Item, where_: &str) -> Result<Vec<String>, String> {
    if let Some(s) = item.as_str() {
        return Ok(vec![s.to_owned()]);
    }
    item.as_array()
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_owned)).collect())
        .ok_or_else(|| format!("{where_}: expected a string or a list of strings"))
}

fn as_number(item: &Item) -> Option<f64> {
    item.as_float()
        .or_else(|| item.as_integer().map(|i| i as f64))
        .filter(|v| v.is_finite())
}

/// A list of finite numbers.
fn numbers(item: &Item) -> Option<Vec<f64>> {
    item.as_array()?
        .iter()
        .map(|v| {
            v.as_float()
                .or_else(|| v.as_integer().map(|i| i as f64))
                .filter(|v| v.is_finite())
        })
        .collect()
}

fn optional(table: &dyn TableLike, key: &str, where_: &str) -> Result<Option<f64>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(item) => as_number(item)
            .map(Some)
            .ok_or_else(|| format!("{where_}: `{key}` must be a finite number")),
    }
}

fn number(table: &dyn TableLike, key: &str, where_: &str) -> Result<f64, String> {
    optional(table, key, where_)?.ok_or_else(|| format!("{where_}: missing key `{key}`"))
}

/// The result grid of `frame`: the temperature `NT` at each point, and the heat flux `HFL` and
/// the part number `part` (from 1) of each cell.
pub fn frame_grid(mesh: &Mesh, frame: &Frame) -> Grid {
    Grid {
        points: mesh.points.clone(),
        cell_types: vec![VTK_HEXAHEDRON; mesh.cells.len()],
        cells: mesh.cells.iter().map(|c| c.to_vec()).collect(),
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

    const WALL: &str = include_str!("../../../validation/cases/grid-layered-wall/wall/grid.toml");
    const BLOCK: &str =
        include_str!("../../../validation/cases/grid-constrained-block/block/grid.toml");
    const HEATED: &str =
        include_str!("../../../validation/cases/grid-heated-block-stress-ccx/block/grid.toml");

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
}
