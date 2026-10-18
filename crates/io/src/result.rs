//! The result contract of a solver run, which `fh validate` reads: the base grid with its id
//! and set flag arrays, and the element stress rule. `docs/solver-protocol.md`, section
//! "Result contract".

use std::collections::{HashMap, HashSet};

use fh_inp::Model;

use crate::vtu::{Field, Grid, VTK_HEXAHEDRON, VTK_QUADRATIC_TETRA};

/// The grid of `model` without result fields: the points and the cells in model order,
/// `NODE_ID` and `ELEMENT_ID`, and for each set a flag array, `NSET_<name>` on the points and
/// `ELSET_<name>` on the cells. A flag is 1 for a member and 0 for all other entries.
pub fn base_grid(model: &Model) -> Result<Grid, String> {
    let index: HashMap<u32, u32> = (model.nodes.iter().enumerate())
        .map(|(i, n)| (n.id, i as u32))
        .collect();
    let mut grid = Grid {
        points: model.nodes.iter().map(|n| n.xyz).collect(),
        ..Grid::default()
    };
    for el in &model.elements {
        grid.cell_types.push(match el.element_type.as_str() {
            "C3D10" => VTK_QUADRATIC_TETRA,
            "C3D8" | "C3D8I" => VTK_HEXAHEDRON,
            _ => return Err(format!("element {}: unsupported type", el.id)),
        });
        let cell = el
            .nodes
            .iter()
            .map(|n| index.get(n).copied().ok_or(format!("node {n} not found")))
            .collect::<Result<_, _>>()?;
        grid.cells.push(cell);
    }
    let ids = |what: &str, v: &mut dyn Iterator<Item = u32>| -> Result<Vec<i32>, String> {
        v.map(|i| {
            i32::try_from(i).map_err(|_| {
                format!(
                    "{what} {i} is above {}: the 32-bit {what} array of the VTU file cannot hold it",
                    i32::MAX
                )
            })
        })
        .collect()
    };
    let node_ids = ids("node id", &mut model.nodes.iter().map(|n| n.id))?;
    grid.point_data.push(Field::i32("NODE_ID", node_ids));
    let element_ids = ids("element id", &mut model.elements.iter().map(|e| e.id))?;
    grid.cell_data.push(Field::i32("ELEMENT_ID", element_ids));
    for s in &model.nsets {
        let member: HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .nodes
            .iter()
            .map(|n| i32::from(member.contains(&n.id)));
        grid.point_data
            .push(Field::i32(&format!("NSET_{}", s.name), flags.collect()));
    }
    for s in &model.elsets {
        let member: HashSet<u32> = s.ids.iter().copied().collect();
        let flags = model
            .elements
            .iter()
            .map(|e| i32::from(member.contains(&e.id)));
        grid.cell_data
            .push(Field::i32(&format!("ELSET_{}", s.name), flags.collect()));
    }
    Ok(grid)
}

/// The element stress `S`, `[xx, yy, zz, xy, yz, xz]`: the mean of the integration point
/// stress, with each point weighted by its integration weight times its Jacobian determinant.
/// This is the volume mean. `points` gives the stress and the weight times Jacobian of each point.
pub fn element_stress(points: impl IntoIterator<Item = ([f64; 6], f64)>) -> [f64; 6] {
    let (mut sum, mut volume) = ([0.0; 6], 0.0);
    for (stress, w) in points {
        for (m, s) in sum.iter_mut().zip(stress) {
            *m += w * s;
        }
        volume += w;
    }
    sum.map(|v| v / volume)
}
