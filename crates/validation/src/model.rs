//! The optional `[model]` table of `case.toml`: what `fh model` needs to turn a `.msh` file
//! into a solver input. Sets are the nodes on a coordinate plane, so no named regions exist.

use toml_edit::{ArrayOfTables, Item, Table};

use crate::fixture::{FixtureError, FixtureReader};

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialSpec {
    pub name: String,
    pub youngs_modulus: f64,
    pub poissons_ratio: f64,
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
}

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
    pub material: MaterialSpec,
    pub sets: Vec<SetSpec>,
    pub boundaries: Vec<BoundarySpec>,
    pub cloads: Vec<CloadSpec>,
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
            &["step", "material", "set", "boundary", "cload"],
        )?;
        let step = if table.contains_key("step") {
            self.string(table, "step")?
        } else {
            "LOAD".to_owned()
        };
        let m_item = self.get(table, "material")?;
        let m = m_item
            .as_table()
            .ok_or_else(|| self.err(Some(m_item), "`model.material` must be a table".into()))?;
        self.only_keys(
            m,
            "model.material",
            &["name", "youngs_modulus", "poissons_ratio"],
        )?;
        let material = MaterialSpec {
            name: self.string(m, "name")?,
            youngs_modulus: self.number(m, "youngs_modulus")?,
            poissons_ratio: self.number(m, "poissons_ratio")?,
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
        let mut sets: Vec<SetSpec> = Vec::new();
        for t in self.rows(table, "set")? {
            self.only_keys(t, "model.set", &["name", "axis", "at", "tol"])?;
            let name = self.string(t, "name")?;
            if name.eq_ignore_ascii_case("ALL")
                || sets.iter().any(|s| s.name.eq_ignore_ascii_case(&name))
            {
                return Err(self.err(
                    t.get("name"),
                    format!("set name `{name}` is reserved or repeated"),
                ));
            }
            let axis_name = self.string(t, "axis")?;
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
                at: self.number(t, "at")?,
                tol,
            });
        }
        let known = |at: Option<&Item>, set: &str| -> Result<(), FixtureError> {
            if sets.iter().any(|s| s.name.eq_ignore_ascii_case(set)) {
                Ok(())
            } else {
                Err(self.err(at, format!("`{set}` is not a `[[model.set]]` name")))
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
        if boundaries.is_empty() {
            return Err(self.err(
                Some(item),
                "`model` needs at least one `[[model.boundary]]`".into(),
            ));
        }
        Ok(Some(ModelSpec {
            step,
            material,
            sets,
            boundaries,
            cloads,
        }))
    }
}
