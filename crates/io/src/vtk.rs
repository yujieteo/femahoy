//! Legacy VTK writer: ASCII `UNSTRUCTURED_GRID`, version 3.0.
//!
//! The writer takes the same [`Grid`] as the VTU writer. It writes one array at a time, one
//! line for each point or cell, so it makes no copy of a field. Reals use the shortest text
//! that reads back to the same value.

use std::io::Write;

use crate::vtu::{Field, Grid, Values, VtuError};

fn bad<T>(msg: String) -> Result<T, VtuError> {
    Err(VtuError::new(msg))
}

/// Write `f64` so that it reads back exactly. Very small and very large values use an exponent.
fn real(w: &mut impl Write, v: f64) -> std::io::Result<()> {
    if v != 0.0 && !(1e-5..1e15).contains(&v.abs()) {
        write!(w, "{v:e}")
    } else {
        write!(w, "{v}")
    }
}

fn check(grid: &Grid) -> Result<(), VtuError> {
    if grid.cell_types.len() != grid.cells.len() {
        return bad("cell_types and cells differ in length".into());
    }
    for (what, fields, n) in [
        ("point", &grid.point_data, grid.points.len()),
        ("cell", &grid.cell_data, grid.cells.len()),
    ] {
        for f in fields {
            let len = match &f.values {
                Values::F64(v) => v.len(),
                Values::I32(v) => v.len(),
            };
            if f.components == 0 || len != n * f.components {
                return bad(format!(
                    "{what} field {} has {len} values, expected {n} x {}",
                    f.name, f.components
                ));
            }
            if f.name.contains(char::is_whitespace) {
                return bad(format!("field name `{}` has a space", f.name));
            }
        }
    }
    Ok(())
}

fn field(w: &mut impl Write, f: &Field) -> std::io::Result<()> {
    match &f.values {
        Values::I32(v) => {
            writeln!(w, "SCALARS {} int 1\nLOOKUP_TABLE default", f.name)?;
            for x in v {
                writeln!(w, "{x}")?;
            }
        }
        Values::F64(v) => {
            match f.components {
                1 => writeln!(w, "SCALARS {} double 1\nLOOKUP_TABLE default", f.name)?,
                3 => writeln!(w, "VECTORS {} double", f.name)?,
                c => writeln!(
                    w,
                    "FIELD FieldData 1\n{} {c} {} double",
                    f.name,
                    v.len() / c
                )?,
            }
            for row in v.chunks(f.components) {
                for (i, x) in row.iter().enumerate() {
                    if i > 0 {
                        w.write_all(b" ")?;
                    }
                    real(w, *x)?;
                }
                w.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

/// Write `grid` as a legacy VTK file. `title` is the second line of the file (at most 255
/// characters, one line).
pub fn write_vtk_to<W: Write>(grid: &Grid, title: &str, w: &mut W) -> Result<(), VtuError> {
    check(grid)?;
    let title: String = title.chars().filter(|c| *c != '\n').take(255).collect();
    let io = |e: std::io::Error| VtuError::new(e.to_string());
    (|| -> std::io::Result<()> {
        write!(
            w,
            "# vtk DataFile Version 3.0\n{title}\nASCII\nDATASET UNSTRUCTURED_GRID\nPOINTS {} double\n",
            grid.points.len()
        )?;
        for p in &grid.points {
            real(w, p[0])?;
            w.write_all(b" ")?;
            real(w, p[1])?;
            w.write_all(b" ")?;
            real(w, p[2])?;
            w.write_all(b"\n")?;
        }
        let size: usize = grid.cells.iter().map(|c| c.len() + 1).sum();
        writeln!(w, "CELLS {} {size}", grid.cells.len())?;
        for c in &grid.cells {
            write!(w, "{}", c.len())?;
            for i in c {
                write!(w, " {i}")?;
            }
            w.write_all(b"\n")?;
        }
        writeln!(w, "CELL_TYPES {}", grid.cell_types.len())?;
        for t in &grid.cell_types {
            writeln!(w, "{t}")?;
        }
        if !grid.point_data.is_empty() {
            writeln!(w, "POINT_DATA {}", grid.points.len())?;
            for f in &grid.point_data {
                field(w, f)?;
            }
        }
        if !grid.cell_data.is_empty() {
            writeln!(w, "CELL_DATA {}", grid.cells.len())?;
            for f in &grid.cell_data {
                field(w, f)?;
            }
        }
        Ok(())
    })()
    .map_err(io)
}
