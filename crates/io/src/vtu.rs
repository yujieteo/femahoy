//! VTU (VTK unstructured grid) and PVD (collection) files.
//!
//! The writer puts array data in one appended raw block. Each array has a
//! `UInt32` byte count in front, in little endian order. The reader accepts the
//! files that this writer makes.

use std::fmt::{self, Write as _};

/// VTK cell type numbers used by femahoy.
pub const VTK_HEXAHEDRON: u8 = 12;
pub const VTK_QUADRATIC_TETRA: u8 = 24;

/// An error while reading a VTU or PVD file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtuError(String);

impl fmt::Display for VtuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VtuError {}

fn err<T>(message: impl Into<String>) -> Result<T, VtuError> {
    Err(VtuError(message.into()))
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

/// Collects the appended block and the matching XML tags.
struct Appended {
    bytes: Vec<u8>,
}

impl Appended {
    fn array(
        &mut self,
        xml: &mut String,
        name: &str,
        type_name: &str,
        components: usize,
        payload: &[u8],
    ) {
        let offset = self.bytes.len();
        self.bytes
            .extend_from_slice(&(payload.len() as u32).to_le_bytes());
        self.bytes.extend_from_slice(payload);
        let _ = writeln!(
            xml,
            "<DataArray type=\"{type_name}\" Name=\"{name}\" NumberOfComponents=\"{components}\" format=\"appended\" offset=\"{offset}\"/>"
        );
    }

    fn field(&mut self, xml: &mut String, f: &Field) {
        let payload: Vec<u8> = match &f.values {
            Values::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Values::I32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        };
        self.array(xml, &f.name, f.values.type_name(), f.components, &payload);
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

/// Write a grid as a VTU file with appended raw data.
pub fn write_vtu(grid: &Grid) -> Result<Vec<u8>, VtuError> {
    if grid.cells.len() != grid.cell_types.len() {
        return err("cells and cell_types differ in length");
    }
    for (kind, list) in [("point", &grid.point_data), ("cell", &grid.cell_data)] {
        let n = if kind == "point" {
            grid.points.len()
        } else {
            grid.cells.len()
        };
        for f in list {
            if f.components == 0 || f.values.len() != n * f.components {
                return err(format!("{kind} field {} has the wrong length", f.name));
            }
        }
    }
    let mut app = Appended { bytes: Vec::new() };
    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\"?>\n<VTKFile type=\"UnstructuredGrid\" version=\"1.0\" byte_order=\"LittleEndian\" header_type=\"UInt32\">\n<UnstructuredGrid>\n");
    let _ = writeln!(
        xml,
        "<Piece NumberOfPoints=\"{}\" NumberOfCells=\"{}\">",
        grid.points.len(),
        grid.cells.len()
    );
    xml.push_str("<Points>\n");
    let pts: Vec<u8> = grid
        .points
        .iter()
        .flatten()
        .flat_map(|x| x.to_le_bytes())
        .collect();
    app.array(&mut xml, "Points", "Float64", 3, &pts);
    xml.push_str("</Points>\n<Cells>\n");
    let conn: Vec<u8> = grid
        .cells
        .iter()
        .flatten()
        .flat_map(|i| i64::from(*i).to_le_bytes())
        .collect();
    app.array(&mut xml, "connectivity", "Int64", 1, &conn);
    let mut end = 0_i64;
    let offsets: Vec<u8> = grid
        .cells
        .iter()
        .flat_map(|c| {
            end += c.len() as i64;
            end.to_le_bytes()
        })
        .collect();
    app.array(&mut xml, "offsets", "Int64", 1, &offsets);
    app.array(&mut xml, "types", "UInt8", 1, &grid.cell_types);
    xml.push_str("</Cells>\n<PointData>\n");
    for f in &grid.point_data {
        app.field(
            &mut xml,
            &Field {
                name: escape(&f.name),
                ..f.clone()
            },
        );
    }
    xml.push_str("</PointData>\n<CellData>\n");
    for f in &grid.cell_data {
        app.field(
            &mut xml,
            &Field {
                name: escape(&f.name),
                ..f.clone()
            },
        );
    }
    xml.push_str("</CellData>\n</Piece>\n</UnstructuredGrid>\n<AppendedData encoding=\"raw\">\n_");
    let mut out = xml.into_bytes();
    out.extend_from_slice(&app.bytes);
    out.extend_from_slice(b"\n</AppendedData>\n</VTKFile>\n");
    Ok(out)
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

/// Read the block of one array from the appended data.
fn block(data: &[u8], h: &Header) -> Result<Vec<u8>, VtuError> {
    let head = data
        .get(h.offset..h.offset + 4)
        .ok_or_else(|| VtuError(format!("array {} lies outside the file", h.name)))?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    data.get(h.offset + 4..h.offset + 4 + len)
        .map(<[u8]>::to_vec)
        .ok_or_else(|| VtuError(format!("array {} is cut short", h.name)))
}

fn ints(h: &Header, raw: &[u8]) -> Result<Vec<i64>, VtuError> {
    let width = match h.type_name.as_str() {
        "Int64" => 8,
        "Int32" => 4,
        "UInt8" => 1,
        t => return err(format!("array {}: unsupported type {t}", h.name)),
    };
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
        .ok_or_else(|| VtuError("no raw appended data".into()))?;
    let text = String::from_utf8_lossy(&bytes[..at]).into_owned();
    let mut start = at + marker.len();
    while bytes.get(start) != Some(&b'_') {
        start += 1;
        if start >= bytes.len() {
            return err("appended data has no start mark");
        }
    }
    let data = &bytes[start + 1..];
    let piece = text
        .find("<Piece")
        .ok_or_else(|| VtuError("no Piece tag".into()))?;
    let piece_tag = &text[piece..piece + text[piece..].find('>').unwrap_or(0)];
    let count = |key: &str| -> Result<usize, VtuError> {
        attr(piece_tag, key)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| VtuError(format!("Piece has no valid {key}")))
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
        } else if tag.starts_with("DataArray") {
            let tag = format!(" {}", &tag["DataArray".len()..]);
            let get = |k: &str| attr(&tag, k).map(str::to_owned);
            headers.push(Header {
                section: section.clone(),
                name: unescape(&get("Name").unwrap_or_default()),
                type_name: get("type").unwrap_or_default(),
                components: get("NumberOfComponents")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
                offset: get("offset")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| VtuError("DataArray has no offset".into()))?,
            });
        }
    }
    let find = |sec: &str, name: &str| {
        headers
            .iter()
            .find(|h| h.section == sec && h.name == name)
            .ok_or_else(|| VtuError(format!("array {name} is missing")))
    };
    let mut grid = Grid::default();
    let p = find("Points", "Points")?;
    if p.type_name != "Float64" {
        return err("points must be Float64");
    }
    let raw = block(data, p)?;
    let flat = floats(&raw);
    if flat.len() != 3 * n_points {
        return err("point count does not match the Piece tag");
    }
    grid.points = flat.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
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
        return err("cell count does not match the Piece tag");
    }
    let mut from = 0;
    for &end in &offsets {
        let end = usize::try_from(end).unwrap_or(usize::MAX);
        let ids = conn
            .get(from..end)
            .ok_or_else(|| VtuError("cell offsets are not valid".into()))?;
        grid.cells.push(ids.iter().map(|&i| i as u32).collect());
        from = end;
    }
    grid.cell_types = types.iter().map(|&t| t as u8).collect();
    for h in headers.iter().filter(|h| h.section.ends_with("Data")) {
        let raw = block(data, h)?;
        let values = match h.type_name.as_str() {
            "Float64" => Values::F64(floats(&raw)),
            "Int32" => Values::I32(ints(h, &raw)?.into_iter().map(|v| v as i32).collect()),
            t => return err(format!("array {}: unsupported type {t}", h.name)),
        };
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

fn floats(raw: &[u8]) -> Vec<f64> {
    raw.chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap_or([0; 8])))
        .collect()
}

/// One entry of a PVD collection: the time value and the file name.
#[derive(Debug, Clone, PartialEq)]
pub struct PvdEntry {
    pub time: f64,
    pub file: String,
}

/// Write a PVD collection file.
pub fn write_pvd(entries: &[PvdEntry]) -> String {
    let mut s = String::from("<?xml version=\"1.0\"?>\n<VTKFile type=\"Collection\" version=\"0.1\" byte_order=\"LittleEndian\">\n<Collection>\n");
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
        return err("not a PVD collection file");
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
                _ => err("DataSet needs timestep and file"),
            }
        })
        .collect()
}
