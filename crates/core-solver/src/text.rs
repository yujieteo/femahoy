//! Plain text results: one readable file for each step.
//!
//! The file has a version line, a short header, and three tables with one row for each node,
//! each element and each integration point. The writer takes rows one at a time from a
//! buffered file, so it makes no copy of a result array. Reals use the shortest text that
//! reads back to the same value.

use std::io::{self, Write};

use fh_io::vtu::VTK_QUADRATIC_TETRA;

use crate::StepResult;

/// First line of every text results file.
pub const TEXT_HEADER: &str = "fh-results 1";

fn real(w: &mut impl Write, v: f64) -> io::Result<()> {
    if v != 0.0 && !(1e-5..1e15).contains(&v.abs()) {
        write!(w, " {v:e}")
    } else {
        write!(w, " {v}")
    }
}

/// Number of integration points of a cell of VTK type `cell_type`.
pub fn points_of(cell_type: u8) -> usize {
    if cell_type == VTK_QUADRATIC_TETRA {
        4
    } else {
        8
    }
}

/// Write the text file of one step. `index` counts from 0. `node_ids` and `element_ids` are the
/// ids of the input file, in model order. `cell_types` has the VTK type of each element.
pub fn write_text(
    w: &mut impl Write,
    index: usize,
    step: &StepResult,
    node_ids: &[i32],
    element_ids: &[i32],
    cell_types: &[u8],
) -> io::Result<()> {
    writeln!(w, "{TEXT_HEADER}")?;
    writeln!(
        w,
        "units: none declared; all values are in the unit system of the input file"
    )?;
    writeln!(w, "step: {}", index + 1)?;
    if let Some(name) = &step.name {
        writeln!(w, "step-name: {name}")?;
    }
    write!(w, "time:")?;
    real(w, step.time)?;
    writeln!(
        w,
        "\nstress-order: S11 S22 S33 S12 S23 S13 (S12 is xy, S23 is yz, S13 is xz)"
    )?;
    writeln!(w)?;

    let with_rotation = step.rotation.iter().flatten().any(|v| *v != 0.0);
    writeln!(w, "nodes: {}", node_ids.len())?;
    write!(w, "node_id U1 U2 U3 RF1 RF2 RF3")?;
    if with_rotation {
        write!(w, " UR1 UR2 UR3")?;
    }
    writeln!(w)?;
    for (i, id) in node_ids.iter().enumerate() {
        write!(w, "{id}")?;
        for v in step.displacement[i].iter().chain(&step.reaction[i]) {
            real(w, *v)?;
        }
        if with_rotation {
            for v in &step.rotation[i] {
                real(w, *v)?;
            }
        }
        writeln!(w)?;
    }
    writeln!(w)?;

    writeln!(w, "elements: {}", element_ids.len())?;
    writeln!(w, "element_id S11 S22 S33 S12 S23 S13 MISES")?;
    for (i, id) in element_ids.iter().enumerate() {
        write!(w, "{id}")?;
        for v in &step.element_stress[i] {
            real(w, *v)?;
        }
        real(w, step.mises[i])?;
        writeln!(w)?;
    }

    if !step.ip_stress.is_empty() {
        writeln!(w)?;
        writeln!(w, "integration-points: {}", step.ip_stress.len())?;
        writeln!(w, "element_id ip S11 S22 S33 S12 S23 S13 MISES")?;
        let mut at = 0;
        for (id, ty) in element_ids.iter().zip(cell_types) {
            for ip in 0..points_of(*ty) {
                let s = &step.ip_stress[at];
                at += 1;
                write!(w, "{id} {}", ip + 1)?;
                for v in s {
                    real(w, *v)?;
                }
                real(w, crate::element::von_mises(s))?;
                writeln!(w)?;
            }
        }
    }
    Ok(())
}
