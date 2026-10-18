//! The optional `[model]` table of `case.toml`: what `fh model` needs to turn a `.msh` file
//! into a solver input. A set is the nodes on a coordinate plane. A region is a named group of
//! faces of the part: `fh mesh` gives it a gmsh physical group (M2b).

use toml_edit::{ArrayOfTables, Item, Table};

use crate::fixture::{FixtureError, FixtureReader};

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialSpec {
    pub name: String,
    pub youngs_modulus: f64,
    pub poissons_ratio: f64,
    pub expansion: Option<f64>,
    /// A volume selector. None retains the single-material ALL section.
    pub bounds: Option<[f64; 6]>,
}

/// A node set: the nodes with `coordinate[axis]` within `tol` of `at`. `tol` is `None` when the
/// case leaves it out, and the command then uses a small part of the mesh size.
#[derive(Debug, Clone, PartialEq)]
pub struct SetSpec {
    pub name: String,
    /// 0 for x, 1 for y, 2 for z.
    pub axis: usize,
    pub at: f64,
    pub tol: Option<f64>,
    pub point: Option<[f64; 3]>,
}

/// How a region selects the faces of the part.
#[derive(Debug, Clone, PartialEq)]
pub enum Select {
    /// The faces on the plane `coordinate[axis] = at`. `axis` is 0 for x, 1 for y, 2 for z.
    Plane { axis: usize, at: f64 },
    /// The faces that lie fully inside the box.
    Box { min: [f64; 3], max: [f64; 3] },
}

/// A named region: faces of the part. `fh mesh` makes a gmsh physical surface of the name.
/// `fh model` writes it as a node set and an element-face surface of the same name.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionSpec {
    pub name: String,
    pub select: Select,
}

/// A uniform pressure on a region (`*DSLOAD`). Positive pressure pushes into the body.
#[derive(Debug, Clone, PartialEq)]
pub struct DsloadSpec {
    pub surface: String,
    pub pressure: f64,
}

/// Names that `fh` uses itself: the element set of the input and the volume group of the mesh.
pub const RESERVED_NAMES: [&str; 2] = ["ALL", "SOLID"];

#[derive(Debug, Clone, PartialEq)]
pub struct BoundarySpec {
    pub set: String,
    pub first_dof: u8,
    pub last_dof: u8,
    pub value: f64,
}

/// A force on every node of the set. It is not a total force.
#[derive(Debug, Clone, PartialEq)]
pub struct CloadSpec {
    pub set: String,
    pub dof: u8,
    pub per_node: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelSpec {
    pub step: String,
    pub materials: Vec<MaterialSpec>,
    pub temperature_change: Option<f64>,
    pub sets: Vec<SetSpec>,
    pub regions: Vec<RegionSpec>,
    pub boundaries: Vec<BoundarySpec>,
    pub cloads: Vec<CloadSpec>,
    pub dsloads: Vec<DsloadSpec>,
}

impl FixtureReader<'_> {
    fn only_keys(&self, table: &Table, what: &str, keys: &[&str]) -> Result<(), FixtureError> {
        for (k, item) in table.iter() {
            if !keys.contains(&k) {
                return Err(self.err(Some(item), format!("unknown key `{k}` in `{what}`")));
            }
        }
        Ok(())
    }

    fn dof(&self, table: &Table, key: &str) -> Result<u8, FixtureError> {
        let item = self.get(table, key)?;
        item.as_integer()
            .filter(|d| (1..=3).contains(d))
            .map(|d| d as u8)
            .ok_or_else(|| self.err(Some(item), format!("`{key}` must be 1, 2 or 3")))
    }

    fn rows<'t>(&self, table: &'t Table, key: &str) -> Result<Vec<&'t Table>, FixtureError> {
        let Some(item) = table.get(key) else {
            return Ok(Vec::new());
        };
        let rows: Option<&ArrayOfTables> = item.as_array_of_tables();
        let rows = rows.ok_or_else(|| {
            self.err(
                Some(item),
                format!("`model.{key}` must be a list of `[[model.{key}]]` tables"),
            )
        })?;
        Ok(rows.iter().collect())
    }

    fn number_or(&self, table: &Table, key: &str, default: f64) -> Result<f64, FixtureError> {
        if table.contains_key(key) {
            self.number(table, key)
        } else {
            Ok(default)
        }
    }

    fn material_box(&self, table: &Table) -> Result<Option<[f64; 6]>, FixtureError> {
        let Some(item) = table.get("box") else {
            return Ok(None);
        };
        let values: Option<Vec<f64>> = item.as_array().and_then(|a| {
            a.iter()
                .map(|v| {
                    v.as_float()
                        .or_else(|| v.as_integer().map(|n| n as f64))
                        .filter(|v| v.is_finite())
                })
                .collect()
        });
        let Some(&[x0, y0, z0, x1, y1, z1]) = values.as_deref() else {
            return Err(self.err(Some(item), "material box needs 6 finite numbers".into()));
        };
        if x0 >= x1 || y0 >= y1 || z0 >= z1 {
            return Err(self.err(Some(item), "material box needs positive extents".into()));
        }
        Ok(Some([x0, y0, z0, x1, y1, z1]))
    }

    /// The `[model]` table, or `None` when the case has none.
    pub(crate) fn model(&self, root: &Table) -> Result<Option<ModelSpec>, FixtureError> {
        let Some(item) = root.get("model") else {
            return Ok(None);
        };
        let table = item
            .as_table()
            .ok_or_else(|| self.err(Some(item), "`model` must be a table".into()))?;
        self.only_keys(
            table,
            "model",
            &[
                "step",
                "material",
                "set",
                "region",
                "boundary",
                "cload",
                "dsload",
                "temperature_change",
            ],
        )?;
        let step = if table.contains_key("step") {
            self.string(table, "step")?
        } else {
            "LOAD".to_owned()
        };
        let m_item = self.get(table, "material")?;
        let tables: Vec<&Table> = if let Some(m) = m_item.as_table() {
            vec![m]
        } else {
            self.rows(table, "material")?
        };
        if tables.is_empty() {
            return Err(self.err(Some(m_item), "model needs a material".into()));
        }
        let mut materials: Vec<MaterialSpec> = Vec::new();
        for m in tables {
            self.only_keys(
                m,
                "model.material",
                &[
                    "name",
                    "youngs_modulus",
                    "poissons_ratio",
                    "expansion",
                    "box",
                ],
            )?;
            let material = MaterialSpec {
                name: self.string(m, "name")?,
                youngs_modulus: self.number(m, "youngs_modulus")?,
                poissons_ratio: self.number(m, "poissons_ratio")?,
                expansion: m
                    .get("expansion")
                    .map(|_| self.number(m, "expansion"))
                    .transpose()?,
                bounds: self.material_box(m)?,
            };
            if material.youngs_modulus <= 0.0 {
                return Err(self.err(
                    m.get("youngs_modulus"),
                    "`youngs_modulus` must be above 0".into(),
                ));
            }
            if !(-1.0 < material.poissons_ratio && material.poissons_ratio < 0.5) {
                return Err(self.err(
                    m.get("poissons_ratio"),
                    "`poissons_ratio` must be above -1 and below 0.5".into(),
                ));
            }
            let name = &material.name;
            let mut chars = name.chars();
            if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
                || name.eq_ignore_ascii_case("ALL")
                || materials.iter().any(|m| m.name.eq_ignore_ascii_case(name))
            {
                return Err(self.err(m.get("name"), "material name is invalid or repeated".into()));
            }
            materials.push(material);
        }
        if materials.len() > 1 && materials.iter().any(|m| m.bounds.is_none()) {
            return Err(self.err(
                Some(m_item),
                "each material needs a box for several materials".into(),
            ));
        }
        let temperature_change = table
            .get("temperature_change")
            .map(|_| self.number(table, "temperature_change"))
            .transpose()?;
        if temperature_change.is_some() && materials.iter().any(|m| m.expansion.is_none()) {
            return Err(self.err(Some(m_item), "each thermal material needs expansion".into()));
        }
        let mut sets: Vec<SetSpec> = Vec::new();
        for t in self.rows(table, "set")? {
            self.only_keys(t, "model.set", &["name", "axis", "at", "tol", "point"])?;
            let name = self.string(t, "name")?;
            if name.eq_ignore_ascii_case("ALL")
                || sets.iter().any(|s| s.name.eq_ignore_ascii_case(&name))
            {
                return Err(self.err(
                    t.get("name"),
                    format!("set name `{name}` is reserved or repeated"),
                ));
            }
            let point = t
                .get("point")
                .map(|item| {
                    if t.contains_key("axis") || t.contains_key("at") || t.contains_key("tol") {
                        return Err(self.err(Some(item), "point cannot use axis, at or tol".into()));
                    }
                    let v: Option<Vec<f64>> = item.as_array().and_then(|a| {
                        a.iter()
                            .map(|v| {
                                v.as_float()
                                    .or_else(|| v.as_integer().map(|x| x as f64))
                                    .filter(|x| x.is_finite())
                            })
                            .collect()
                    });
                    let Some(&[x, y, z]) = v.as_deref() else {
                        return Err(self.err(Some(item), "point needs 3 finite coordinates".into()));
                    };
                    Ok([x, y, z])
                })
                .transpose()?;
            let axis_name = if point.is_some() {
                "x".into()
            } else {
                self.string(t, "axis")?
            };
            let axis = ["x", "y", "z"]
                .iter()
                .position(|a| *a == axis_name)
                .ok_or_else(|| self.err(t.get("axis"), "`axis` must be x, y or z".into()))?;
            let tol = if t.contains_key("tol") {
                let v = self.number(t, "tol")?;
                if v < 0.0 {
                    return Err(self.err(t.get("tol"), "`tol` must be 0 or more".into()));
                }
                Some(v)
            } else {
                None
            };
            sets.push(SetSpec {
                name,
                axis,
                at: if point.is_some() {
                    0.0
                } else {
                    self.number(t, "at")?
                },
                point,
                tol,
            });
        }
        let regions = self.regions(table, &sets)?;
        for m in &materials {
            if m.bounds.is_some()
                && (sets.iter().any(|s| s.name.eq_ignore_ascii_case(&m.name))
                    || regions.iter().any(|r| r.name.eq_ignore_ascii_case(&m.name)))
            {
                return Err(self.err(
                    Some(m_item),
                    "material group name conflicts with a set or surface".into(),
                ));
            }
        }
        let known = |at: Option<&Item>, set: &str| -> Result<(), FixtureError> {
            if sets.iter().any(|s| s.name.eq_ignore_ascii_case(set))
                || regions.iter().any(|r| r.name.eq_ignore_ascii_case(set))
            {
                Ok(())
            } else {
                Err(self.err(
                    at,
                    format!("`{set}` is not a `[[model.set]]` or `[[model.region]]` name"),
                ))
            }
        };
        let mut boundaries = Vec::new();
        for t in self.rows(table, "boundary")? {
            self.only_keys(
                t,
                "model.boundary",
                &["set", "first_dof", "last_dof", "value"],
            )?;
            let set = self.string(t, "set")?;
            known(t.get("set"), &set)?;
            let first_dof = self.dof(t, "first_dof")?;
            let last_dof = if t.contains_key("last_dof") {
                self.dof(t, "last_dof")?
            } else {
                first_dof
            };
            if last_dof < first_dof {
                return Err(self.err(
                    t.get("last_dof"),
                    "`last_dof` must not be below `first_dof`".into(),
                ));
            }
            boundaries.push(BoundarySpec {
                set,
                first_dof,
                last_dof,
                value: self.number_or(t, "value", 0.0)?,
            });
        }
        let mut cloads = Vec::new();
        for t in self.rows(table, "cload")? {
            self.only_keys(t, "model.cload", &["set", "dof", "per_node"])?;
            let set = self.string(t, "set")?;
            known(t.get("set"), &set)?;
            cloads.push(CloadSpec {
                set,
                dof: self.dof(t, "dof")?,
                per_node: self.number(t, "per_node")?,
            });
        }
        let mut dsloads = Vec::new();
        for t in self.rows(table, "dsload")? {
            self.only_keys(t, "model.dsload", &["surface", "pressure"])?;
            let surface = self.string(t, "surface")?;
            if !regions
                .iter()
                .any(|r| r.name.eq_ignore_ascii_case(&surface))
            {
                return Err(self.err(
                    t.get("surface"),
                    format!("`{surface}` is not a `[[model.region]]` name"),
                ));
            }
            dsloads.push(DsloadSpec {
                surface,
                pressure: self.number(t, "pressure")?,
            });
        }
        if boundaries.is_empty() {
            return Err(self.err(
                Some(item),
                "`model` needs at least one `[[model.boundary]]`".into(),
            ));
        }
        Ok(Some(ModelSpec {
            step,
            materials,
            temperature_change,
            sets,
            regions,
            boundaries,
            cloads,
            dsloads,
        }))
    }

    /// The `[[model.region]]` rows. A name is unique among the sets and the regions.
    fn regions(&self, table: &Table, sets: &[SetSpec]) -> Result<Vec<RegionSpec>, FixtureError> {
        let mut regions: Vec<RegionSpec> = Vec::new();
        for t in self.rows(table, "region")? {
            self.only_keys(t, "model.region", &["name", "axis", "at", "box"])?;
            let name = self.string(t, "name")?;
            let taken = |n: &str| name.eq_ignore_ascii_case(n);
            if RESERVED_NAMES.iter().any(|n| taken(n))
                || sets.iter().any(|s| taken(&s.name))
                || regions.iter().any(|r| taken(&r.name))
            {
                return Err(self.err(
                    t.get("name"),
                    format!("region name `{name}` is reserved or repeated"),
                ));
            }
            // The name goes into a gmsh script and an `.inp` file without quotation.
            let mut chars = name.chars();
            if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(self.err(
                    t.get("name"),
                    format!(
                        "region name `{name}` must start with a letter and hold only letters, digits and _"
                    ),
                ));
            }
            let plane = t.contains_key("axis") || t.contains_key("at");
            let select = match t.get("box") {
                Some(item) if plane => {
                    return Err(self.err(
                        Some(item),
                        "a region needs `axis` and `at`, or `box`, not both".into(),
                    ))
                }
                Some(item) => {
                    let numbers: Option<Vec<f64>> = item.as_array().and_then(|a| {
                        a.iter()
                            .map(|v| {
                                v.as_float()
                                    .or_else(|| v.as_integer().map(|i| i as f64))
                                    .filter(|x| x.is_finite())
                            })
                            .collect()
                    });
                    let Some(&[x0, y0, z0, x1, y1, z1]) = numbers.as_deref() else {
                        return Err(self.err(
                            Some(item),
                            "`box` must be 6 numbers: x0, y0, z0, x1, y1, z1".into(),
                        ));
                    };
                    if x0 > x1 || y0 > y1 || z0 > z1 {
                        return Err(self.err(
                            Some(item),
                            "`box` needs x0 <= x1, y0 <= y1 and z0 <= z1".into(),
                        ));
                    }
                    Select::Box {
                        min: [x0, y0, z0],
                        max: [x1, y1, z1],
                    }
                }
                None if plane => {
                    let axis_name = self.string(t, "axis")?;
                    let axis = ["x", "y", "z"]
                        .iter()
                        .position(|a| *a == axis_name)
                        .ok_or_else(|| {
                            self.err(t.get("axis"), "`axis` must be x, y or z".into())
                        })?;
                    Select::Plane {
                        axis,
                        at: self.number(t, "at")?,
                    }
                }
                None => {
                    return Err(self.err(
                        t.get("name"),
                        "a region needs `axis` and `at`, or `box`".into(),
                    ))
                }
            };
            regions.push(RegionSpec { name, select });
        }
        Ok(regions)
    }
}
