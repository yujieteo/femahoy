//! VTU and PVD writers. The reader is in `fh-core` (`vtu` module).
//!
//! The writer puts array data in one appended raw block. Each array has a `UInt32` byte count
//! in front, in little endian order.

use std::fmt::Write as _;

use fh_core::vtu::{Field, Grid, PvdEntry, Values};

fn err<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Where the bytes of one array come from. The writer converts a piece at a
/// time, so it never holds a second copy of an array.
enum Source<'a> {
    Points(&'a [[f64; 3]]),
    Connectivity(&'a [Vec<u32>]),
    Offsets(&'a [Vec<u32>]),
    Bytes(&'a [u8]),
    F64(&'a [f64]),
    I32(&'a [i32]),
}

struct Array<'a> {
    name: String,
    type_name: &'static str,
    components: usize,
    bytes: u64,
    source: Source<'a>,
}

/// The appended data of an array has a `UInt32` byte count in front. A larger
/// array has no valid header, so it is an error.
fn header_len(name: &str, bytes: u64) -> Result<u32, String> {
    u32::try_from(bytes).map_err(|_| {
        format!("array {name} has {bytes} bytes, more than the 4 GiB limit of a UInt32 VTU header")
    })
}

/// Values converted per piece: 64 Ki values is at most 512 KiB.
const PIECE: usize = 1 << 16;

fn emit<T, const N: usize, W: std::io::Write>(
    w: &mut W,
    items: impl Iterator<Item = T>,
    mut bytes: impl FnMut(T) -> [u8; N],
) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(PIECE * N);
    for item in items {
        buf.extend_from_slice(&bytes(item));
        if buf.len() >= PIECE * N {
            w.write_all(&buf)?;
            buf.clear();
        }
    }
    w.write_all(&buf)
}

impl Array<'_> {
    fn write<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        match &self.source {
            Source::Points(p) => emit(w, p.iter().flatten(), |x| x.to_le_bytes()),
            Source::Connectivity(c) => emit(w, c.iter().flatten(), |i| i64::from(*i).to_le_bytes()),
            Source::Offsets(c) => {
                let mut end = 0_i64;
                emit(w, c.iter(), |cell| {
                    end += cell.len() as i64;
                    end.to_le_bytes()
                })
            }
            Source::Bytes(b) => w.write_all(b),
            Source::F64(v) => emit(w, v.iter(), |x| x.to_le_bytes()),
            Source::I32(v) => emit(w, v.iter(), |x| x.to_le_bytes()),
        }
    }
}

fn field_array(f: &Field) -> Array<'_> {
    let (type_name, bytes, source) = match &f.values {
        Values::F64(v) => ("Float64", 8 * v.len() as u64, Source::F64(v)),
        Values::I32(v) => ("Int32", 4 * v.len() as u64, Source::I32(v)),
    };
    Array {
        name: escape(&f.name),
        type_name,
        components: f.components,
        bytes,
        source,
    }
}

/// The first problem that stops a grid from being written.
fn check(grid: &Grid) -> Result<(), String> {
    if grid.cells.len() != grid.cell_types.len() {
        return err("cells and cell_types differ in length");
    }
    for (i, cell) in grid.cells.iter().enumerate() {
        if let Some(p) = cell.iter().find(|p| **p as usize >= grid.points.len()) {
            return err(format!(
                "cell {i} has the point {p}, but the grid has {} points",
                grid.points.len()
            ));
        }
    }
    for (kind, list, n) in [
        ("point", &grid.point_data, grid.points.len()),
        ("cell", &grid.cell_data, grid.cells.len()),
    ] {
        for f in list {
            if f.components == 0 || n.checked_mul(f.components) != Some(f.values.count()) {
                return err(format!("{kind} field {} has the wrong length", f.name));
            }
        }
    }
    Ok(())
}

/// Write a grid as a VTU file with appended raw data.
pub(crate) fn write_vtu(grid: &Grid) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    write_vtu_to(grid, &mut out)?;
    Ok(out)
}

/// Write a grid as a VTU file to `w`, one piece of an array at a time.
fn write_vtu_to<W: std::io::Write>(grid: &Grid, w: &mut W) -> Result<(), String> {
    check(grid)?;
    let n_conn: u64 = grid.cells.iter().map(|c| c.len() as u64).sum();
    let cell = |name: &str, type_name, bytes, source| Array {
        name: name.into(),
        type_name,
        components: 1,
        bytes,
        source,
    };
    let points = Array {
        name: "Points".into(),
        type_name: "Float64",
        components: 3,
        bytes: 24 * grid.points.len() as u64,
        source: Source::Points(&grid.points),
    };
    let cells = [
        cell(
            "connectivity",
            "Int64",
            8 * n_conn,
            Source::Connectivity(&grid.cells),
        ),
        cell(
            "offsets",
            "Int64",
            8 * grid.cells.len() as u64,
            Source::Offsets(&grid.cells),
        ),
        cell(
            "types",
            "UInt8",
            grid.cell_types.len() as u64,
            Source::Bytes(&grid.cell_types),
        ),
    ];
    let point_data: Vec<Array> = grid.point_data.iter().map(field_array).collect();
    let cell_data: Vec<Array> = grid.cell_data.iter().map(field_array).collect();
    // Offsets follow from the sizes, so the XML can be written before the data.
    let mut order: Vec<&Array> = vec![&points];
    order.extend(cells.iter().chain(&point_data).chain(&cell_data));
    let mut offset = 0_u64;
    let mut tag = std::collections::HashMap::new();
    for (i, a) in order.iter().enumerate() {
        header_len(&a.name, a.bytes)?;
        let _ = tag.insert(i, offset);
        offset += 4 + a.bytes;
    }
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\"?>\n<VTKFile type=\"UnstructuredGrid\" version=\"1.0\" byte_order=\"LittleEndian\" header_type=\"UInt32\">\n<UnstructuredGrid>\n");
    let _ = writeln!(
        xml,
        "<Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">",
        grid.points.len(),
        grid.cells.len()
    );
    let mut next = 0;
    let mut tags = |xml: &mut String, list: &[Array]| {
        for a in list {
            let _ = writeln!(
                xml,
                "<DataArray type=\"{}\" Name=\"{}\" NumberOfComponents=\"{}\" format=\"appended\" offset=\"{}\"/>",
                a.type_name, a.name, a.components, tag[&next]
            );
            next += 1;
        }
    };
    xml.push_str("<Points>\n");
    tags(&mut xml, std::slice::from_ref(&points));
    xml.push_str("</Points>\n<Cells>\n");
    tags(&mut xml, &cells);
    xml.push_str("</Cells>\n<PointData>\n");
    tags(&mut xml, &point_data);
    xml.push_str("</PointData>\n<CellData>\n");
    tags(&mut xml, &cell_data);
    xml.push_str("</CellData>\n</Piece>\n</UnstructuredGrid>\n<AppendedData encoding=\"raw\">\n_");
    let io = |e: std::io::Error| format!("write failed: {e}");
    w.write_all(xml.as_bytes()).map_err(io)?;
    for a in order {
        // The length was checked above.
        w.write_all(&(a.bytes as u32).to_le_bytes()).map_err(io)?;
        a.write(w).map_err(io)?;
    }
    w.write_all(b"\n</AppendedData>\n</VTKFile>\n").map_err(io)
}

/// Write a PVD collection file. `time` is the step time at the end of each
/// step. `units` names the unit system of the model, or `unspecified`. A `multibody` collection
/// (the motion of rigid bodies) has the attribute `kind="multibody"`.
pub(crate) fn write_pvd(entries: &[PvdEntry], units: &str, multibody: bool) -> String {
    let kind = if multibody { " kind=\"multibody\"" } else { "" };
    let mut s = format!("<?xml version=\"1.0\"?>\n<VTKFile type=\"Collection\" version=\"0.1\" byte_order=\"LittleEndian\">\n<Collection units=\"{}\"{kind}>\n", escape(units));
    for e in entries {
        let _ = writeln!(
            s,
            "<DataSet timestep=\"{}\" part=\"0\" file=\"{}\"/>",
            e.time,
            escape(&e.file)
        );
    }
    s.push_str("</Collection>\n</VTKFile>\n");
    s
}
