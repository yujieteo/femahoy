//! VTU (VTK unstructured grid) and PVD (collection) files.
//!
//! The writer puts array data in one appended raw block. Each array has a
//! `UInt32` byte count in front, in little endian order. The reader accepts the
//! files that this writer makes.

use std::fmt::{self, Write as _};

use crate::code;

/// VTK cell type numbers of the solid elements that CalculiX writes.
pub const VTK_LINE: u8 = 3;
pub const VTK_TRIANGLE: u8 = 5;
pub const VTK_QUAD: u8 = 9;
pub const VTK_TETRA: u8 = 10;
pub const VTK_HEXAHEDRON: u8 = 12;
pub const VTK_WEDGE: u8 = 13;
pub const VTK_QUADRATIC_EDGE: u8 = 21;
pub const VTK_QUADRATIC_TRIANGLE: u8 = 22;
pub const VTK_QUADRATIC_QUAD: u8 = 23;
pub const VTK_QUADRATIC_TETRA: u8 = 24;
pub const VTK_QUADRATIC_HEXAHEDRON: u8 = 25;
pub const VTK_QUADRATIC_WEDGE: u8 = 26;

/// An error while reading a VTU or PVD file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtuError {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for VtuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl VtuError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        VtuError {
            code,
            message: message.into(),
        }
    }
}

impl std::error::Error for VtuError {}

fn err<T>(code: &'static str, message: impl Into<String>) -> Result<T, VtuError> {
    Err(VtuError::new(code, message))
}

/// Array values. Floats hold fields. Integers hold sets and ids.
#[derive(Debug, Clone, PartialEq)]
pub enum Values {
    F64(Vec<f64>),
    I32(Vec<i32>),
}

impl Values {
    fn len(&self) -> usize {
        match self {
            Values::F64(v) => v.len(),
            Values::I32(v) => v.len(),
        }
    }
    fn type_name(&self) -> &'static str {
        match self {
            Values::F64(_) => "Float64",
            Values::I32(_) => "Int32",
        }
    }
}

/// A named array with one or more components per point or cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub components: usize,
    pub values: Values,
}

impl Field {
    pub fn f64(name: &str, components: usize, values: Vec<f64>) -> Self {
        Field {
            name: name.into(),
            components,
            values: Values::F64(values),
        }
    }
    pub fn i32(name: &str, values: Vec<i32>) -> Self {
        Field {
            name: name.into(),
            components: 1,
            values: Values::I32(values),
        }
    }
    /// The float values, or `None` for an integer array.
    pub fn as_f64(&self) -> Option<&[f64]> {
        match &self.values {
            Values::F64(v) => Some(v),
            Values::I32(_) => None,
        }
    }
}

/// One unstructured grid.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grid {
    pub points: Vec<[f64; 3]>,
    /// VTK cell type of each cell.
    pub cell_types: Vec<u8>,
    /// Point indices of each cell.
    pub cells: Vec<Vec<u32>>,
    pub point_data: Vec<Field>,
    pub cell_data: Vec<Field>,
}

impl Grid {
    pub fn point_field(&self, name: &str) -> Option<&Field> {
        self.point_data.iter().find(|f| f.name == name)
    }
    pub fn cell_field(&self, name: &str) -> Option<&Field> {
        self.cell_data.iter().find(|f| f.name == name)
    }
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
fn header_len(name: &str, bytes: u64) -> Result<u32, VtuError> {
    u32::try_from(bytes).map_err(|_| {
        VtuError::new(
            code::VTU_UNSUPPORTED,
            format!(
                "array {name} has {bytes} bytes, more than the 4 GiB limit of a UInt32 VTU header"
            ),
        )
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
    let (bytes, source) = match &f.values {
        Values::F64(v) => (8 * v.len() as u64, Source::F64(v)),
        Values::I32(v) => (4 * v.len() as u64, Source::I32(v)),
    };
    Array {
        name: escape(&f.name),
        type_name: f.values.type_name(),
        components: f.components,
        bytes,
        source,
    }
}

/// The first problem that stops a grid from being written.
pub(crate) fn check(grid: &Grid) -> Result<(), VtuError> {
    if grid.cells.len() != grid.cell_types.len() {
        return err(
            code::VTU_INVALID_GRID,
            "cells and cell_types differ in length",
        );
    }
    for (i, cell) in grid.cells.iter().enumerate() {
        if let Some(p) = cell.iter().find(|p| **p as usize >= grid.points.len()) {
            return err(
                code::VTU_INVALID_GRID,
                format!(
                    "cell {i} has the point {p}, but the grid has {} points",
                    grid.points.len()
                ),
            );
        }
    }
    for (kind, list, n) in [
        ("point", &grid.point_data, grid.points.len()),
        ("cell", &grid.cell_data, grid.cells.len()),
    ] {
        for f in list {
            if f.components == 0 || n.checked_mul(f.components) != Some(f.values.len()) {
                return err(
                    code::VTU_INVALID_GRID,
                    format!("{kind} field {} has the wrong length", f.name),
                );
            }
        }
    }
    Ok(())
}

/// Write a grid as a VTU file with appended raw data.
pub fn write_vtu(grid: &Grid) -> Result<Vec<u8>, VtuError> {
    let mut out = Vec::new();
    write_vtu_to(grid, &mut out)?;
    Ok(out)
}

/// Write a grid as a VTU file to `w`, one piece of an array at a time.
pub fn write_vtu_to<W: std::io::Write>(grid: &Grid, w: &mut W) -> Result<(), VtuError> {
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
    let io =
        |e: std::io::Error| VtuError::new(code::VTU_WRITE_FAILED, format!("write failed: {e}"));
    w.write_all(xml.as_bytes()).map_err(io)?;
    for a in order {
        // The length was checked above.
        w.write_all(&(a.bytes as u32).to_le_bytes()).map_err(io)?;
        a.write(w).map_err(io)?;
    }
    w.write_all(b"\n</AppendedData>\n</VTKFile>\n").map_err(io)
}

/// The value of an attribute in a tag text.
fn attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!(" {key}=\"");
    let start = tag.find(&pat)? + pat.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

struct Header {
    section: String,
    name: String,
    type_name: String,
    components: usize,
    offset: usize,
}

/// The appended data ends with this text, right after the last block.
const FOOTER: &[u8] = b"\n</AppendedData>";

/// The end of the block of one array in the appended data.
fn block_end(data: &[u8], h: &Header) -> Result<usize, VtuError> {
    let outside = || {
        VtuError::new(
            code::VTU_MALFORMED,
            format!("array {} lies outside the file", h.name),
        )
    };
    let start = h.offset.checked_add(4).ok_or_else(outside)?;
    let head = data.get(h.offset..start).ok_or_else(outside)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    start
        .checked_add(len)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| {
            VtuError::new(
                code::VTU_MALFORMED,
                format!("array {} is cut short", h.name),
            )
        })
}

/// Read the block of one array from the appended data.
fn block(data: &[u8], h: &Header) -> Result<Vec<u8>, VtuError> {
    Ok(data[h.offset + 4..block_end(data, h)?].to_vec())
}

/// A block that ends in part of a value is damaged.
fn whole(h: &Header, raw: &[u8], width: usize) -> Result<(), VtuError> {
    if raw.len().is_multiple_of(width) {
        Ok(())
    } else {
        err(
            code::VTU_MALFORMED,
            format!("array {} ends in an incomplete value", h.name),
        )
    }
}

fn ints(h: &Header, raw: &[u8]) -> Result<Vec<i64>, VtuError> {
    let width = match h.type_name.as_str() {
        "Int64" => 8,
        "Int32" => 4,
        "UInt8" => 1,
        t => {
            return err(
                code::VTU_UNSUPPORTED,
                format!("array {}: unsupported type {t}", h.name),
            )
        }
    };
    whole(h, raw, width)?;
    Ok(raw
        .chunks_exact(width)
        .map(|c| match width {
            8 => i64::from_le_bytes(c.try_into().unwrap_or([0; 8])),
            4 => i64::from(i32::from_le_bytes(c.try_into().unwrap_or([0; 4]))),
            _ => i64::from(c[0]),
        })
        .collect())
}

/// Read a VTU file made by `write_vtu`.
pub fn read_vtu(bytes: &[u8]) -> Result<Grid, VtuError> {
    let marker = b"<AppendedData encoding=\"raw\">";
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, "no raw appended data"))?;
    let text = String::from_utf8_lossy(&bytes[..at]).into_owned();
    let mut start = at + marker.len();
    while bytes.get(start) != Some(&b'_') {
        start += 1;
        if start >= bytes.len() {
            return err(code::VTU_MALFORMED, "appended data has no start mark");
        }
    }
    let data = &bytes[start + 1..];
    let piece = text
        .find("<Piece")
        .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, "no Piece tag"))?;
    let piece_tag = &text[piece..piece + text[piece..].find('>').unwrap_or(0)];
    let count = |key: &str| -> Result<usize, VtuError> {
        attr(piece_tag, key)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, format!("Piece has no valid {key}")))
    };
    let (n_points, n_cells) = (count("NumberOfPoints")?, count("NumberOfCells")?);
    let mut headers = Vec::new();
    let mut section = String::new();
    for tag in text.split('<').skip(1) {
        let tag = tag.trim_end();
        if let Some(name) = tag.strip_suffix('>').and_then(|t| t.strip_prefix('/')) {
            if name == section {
                section.clear();
            }
        } else if let Some(name) = ["Points", "Cells", "PointData", "CellData"]
            .into_iter()
            .find(|n| tag.strip_prefix(*n).is_some_and(|r| r.starts_with('>')))
        {
            section = name.into();
        } else if let Some(rest) = tag.strip_prefix("DataArray") {
            let tag = format!(" {rest}");
            let get = |k: &str| attr(&tag, k).map(str::to_owned);
            headers.push(Header {
                section: section.clone(),
                name: unescape(&get("Name").unwrap_or_default()),
                type_name: get("type").unwrap_or_default(),
                components: match get("NumberOfComponents") {
                    None => 1,
                    Some(v) => v.parse().ok().filter(|c| *c > 0).ok_or_else(|| {
                        VtuError::new(
                            code::VTU_MALFORMED,
                            format!("DataArray has a bad NumberOfComponents {v}"),
                        )
                    })?,
                },
                offset: get("offset")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, "DataArray has no offset"))?,
            });
        }
    }
    // The footer comes right after the last block. Thus a block that runs into the footer,
    // a payload that is cut short, and a file without its closing tags are errors.
    let mut end = 0;
    for h in &headers {
        end = end.max(block_end(data, h)?);
    }
    let rest = data[end..].strip_prefix(FOOTER).ok_or_else(|| {
        VtuError::new(
            code::VTU_MALFORMED,
            "the appended data does not end after its last array",
        )
    })?;
    if rest.trim_ascii() != b"</VTKFile>" {
        return err(code::VTU_MALFORMED, "the file does not end with </VTKFile>");
    }
    let find = |sec: &str, name: &str| {
        headers
            .iter()
            .find(|h| h.section == sec && h.name == name)
            .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, format!("array {name} is missing")))
    };
    let mut grid = Grid::default();
    let p = find("Points", "Points")?;
    if p.type_name != "Float64" {
        return err(code::VTU_MALFORMED, "points must be Float64");
    }
    let raw = block(data, p)?;
    let flat = floats(p, &raw)?;
    if n_points.checked_mul(3) != Some(flat.len()) {
        return err(
            code::VTU_MALFORMED,
            "point count does not match the Piece tag",
        );
    }
    grid.points = flat.as_chunks::<3>().0.to_vec();
    let conn = ints(
        find("Cells", "connectivity")?,
        &block(data, find("Cells", "connectivity")?)?,
    )?;
    let offsets = ints(
        find("Cells", "offsets")?,
        &block(data, find("Cells", "offsets")?)?,
    )?;
    let types = ints(
        find("Cells", "types")?,
        &block(data, find("Cells", "types")?)?,
    )?;
    if offsets.len() != n_cells || types.len() != n_cells {
        return err(
            code::VTU_MALFORMED,
            "cell count does not match the Piece tag",
        );
    }
    let mut from = 0;
    for &end in &offsets {
        let end = usize::try_from(end).unwrap_or(usize::MAX);
        let ids = conn
            .get(from..end)
            .ok_or_else(|| VtuError::new(code::VTU_MALFORMED, "cell offsets are not valid"))?;
        let point = |&i: &i64| {
            u32::try_from(i)
                .ok()
                .filter(|p| (*p as usize) < n_points)
                .ok_or_else(|| {
                    VtuError::new(
                        code::VTU_MALFORMED,
                        format!("a cell has the point {i}, not a point of the grid"),
                    )
                })
        };
        grid.cells
            .push(ids.iter().map(point).collect::<Result<_, _>>()?);
        from = end;
    }
    if from != conn.len() {
        return err(
            code::VTU_MALFORMED,
            "the last cell offset does not end the connectivity",
        );
    }
    grid.cell_types = types
        .iter()
        .map(|&t| {
            u8::try_from(t).map_err(|_| {
                VtuError::new(code::VTU_MALFORMED, format!("cell type {t} is not valid"))
            })
        })
        .collect::<Result<_, _>>()?;
    for h in headers.iter().filter(|h| h.section.ends_with("Data")) {
        let raw = block(data, h)?;
        let values = match h.type_name.as_str() {
            "Float64" => Values::F64(floats(h, &raw)?),
            "Int32" => Values::I32(ints(h, &raw)?.into_iter().map(|v| v as i32).collect()),
            t => {
                return err(
                    code::VTU_UNSUPPORTED,
                    format!("array {}: unsupported type {t}", h.name),
                )
            }
        };
        let n = if h.section == "PointData" {
            n_points
        } else {
            n_cells
        };
        if n.checked_mul(h.components) != Some(values.len()) {
            return err(
                code::VTU_MALFORMED,
                format!(
                    "array {} has {} values, not {n} x {}",
                    h.name,
                    values.len(),
                    h.components
                ),
            );
        }
        let field = Field {
            name: h.name.clone(),
            components: h.components,
            values,
        };
        if h.section == "PointData" {
            grid.point_data.push(field);
        } else {
            grid.cell_data.push(field);
        }
    }
    Ok(grid)
}

fn floats(h: &Header, raw: &[u8]) -> Result<Vec<f64>, VtuError> {
    whole(h, raw, 8)?;
    Ok(raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| f64::from_le_bytes(*c))
        .collect())
}

/// One entry of a PVD collection: the time value and the file name.
#[derive(Debug, Clone, PartialEq)]
pub struct PvdEntry {
    pub time: f64,
    pub file: String,
}

/// Write a PVD collection file with the units `unspecified`.
pub fn write_pvd(entries: &[PvdEntry]) -> String {
    write_pvd_units(entries, "unspecified")
}

/// Write a PVD collection file. `time` is the step time at the end of each
/// step. `units` names the unit system of the model, or `unspecified`.
pub fn write_pvd_units(entries: &[PvdEntry], units: &str) -> String {
    let mut s = format!("<?xml version=\"1.0\"?>\n<VTKFile type=\"Collection\" version=\"0.1\" byte_order=\"LittleEndian\">\n<Collection units=\"{}\">\n", escape(units));
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

/// Read a PVD collection file.
pub fn read_pvd(text: &str) -> Result<Vec<PvdEntry>, VtuError> {
    if !text.contains("type=\"Collection\"") {
        return err(code::VTU_MALFORMED, "not a PVD collection file");
    }
    text.split('<')
        .filter(|t| t.starts_with("DataSet"))
        .map(|t| {
            let t = format!(" {}", &t["DataSet".len()..]);
            let time = attr(&t, "timestep").and_then(|v| v.parse().ok());
            let file = attr(&t, "file");
            match (time, file) {
                (Some(time), Some(file)) => Ok(PvdEntry {
                    time,
                    file: unescape(file),
                }),
                _ => err(code::VTU_MALFORMED, "DataSet needs timestep and file"),
            }
        })
        .collect()
}
