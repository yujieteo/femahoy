//! VTU (VTK unstructured grid) and PVD (collection) files.
//!
//! The reader accepts the files that the writer of `fh` makes (`fh/src/vtu.rs`): array data
//! in one appended raw block, each array with a `UInt32` byte count in front, in little
//! endian order.

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

fn err<T>(message: impl Into<String>) -> Result<T, String> {
    Err(message.into())
}

/// Array values. Floats hold fields. Integers hold sets and ids.
#[derive(Debug, Clone, PartialEq)]
pub enum Values {
    F64(Vec<f64>),
    I32(Vec<i32>),
}

impl Values {
    pub fn count(&self) -> usize {
        match self {
            Values::F64(v) => v.len(),
            Values::I32(v) => v.len(),
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
fn block_end(data: &[u8], h: &Header) -> Result<usize, String> {
    let outside = || format!("array {} lies outside the file", h.name);
    let start = h.offset.checked_add(4).ok_or_else(outside)?;
    let head = data.get(h.offset..start).ok_or_else(outside)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    start
        .checked_add(len)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| format!("array {} is cut short", h.name))
}

/// Read the block of one array from the appended data.
fn block(data: &[u8], h: &Header) -> Result<Vec<u8>, String> {
    Ok(data[h.offset + 4..block_end(data, h)?].to_vec())
}

/// A block that ends in part of a value is damaged.
fn whole(h: &Header, raw: &[u8], width: usize) -> Result<(), String> {
    if raw.len().is_multiple_of(width) {
        Ok(())
    } else {
        err(format!("array {} ends in an incomplete value", h.name))
    }
}

fn ints(h: &Header, raw: &[u8]) -> Result<Vec<i64>, String> {
    let width = match h.type_name.as_str() {
        "Int64" => 8,
        "Int32" => 4,
        "UInt8" => 1,
        t => return err(format!("array {}: unsupported type {t}", h.name)),
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
pub fn read_vtu(bytes: &[u8]) -> Result<Grid, String> {
    let marker = b"<AppendedData encoding=\"raw\">";
    let at = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .ok_or_else(|| "no raw appended data".to_owned())?;
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
        .ok_or_else(|| "no Piece tag".to_owned())?;
    let piece_tag = &text[piece..piece + text[piece..].find('>').unwrap_or(0)];
    let count = |key: &str| -> Result<usize, String> {
        attr(piece_tag, key)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("Piece has no valid {key}"))
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
                    Some(v) => v
                        .parse()
                        .ok()
                        .filter(|c| *c > 0)
                        .ok_or_else(|| format!("DataArray has a bad NumberOfComponents {v}"))?,
                },
                offset: get("offset")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| "DataArray has no offset".to_owned())?,
            });
        }
    }
    // The footer comes right after the last block. Thus a block that runs into the footer,
    // a payload that is cut short, and a file without its closing tags are errors.
    let mut end = 0;
    for h in &headers {
        end = end.max(block_end(data, h)?);
    }
    let rest = data[end..]
        .strip_prefix(FOOTER)
        .ok_or_else(|| "the appended data does not end after its last array".to_owned())?;
    if rest.trim_ascii() != b"</VTKFile>" {
        return err("the file does not end with </VTKFile>");
    }
    let find = |sec: &str, name: &str| {
        headers
            .iter()
            .find(|h| h.section == sec && h.name == name)
            .ok_or_else(|| format!("array {name} is missing"))
    };
    let mut grid = Grid::default();
    let p = find("Points", "Points")?;
    if p.type_name != "Float64" {
        return err("points must be Float64");
    }
    let raw = block(data, p)?;
    let flat = floats(p, &raw)?;
    if n_points.checked_mul(3) != Some(flat.len()) {
        return err("point count does not match the Piece tag");
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
        return err("cell count does not match the Piece tag");
    }
    let mut from = 0;
    for &end in &offsets {
        let end = usize::try_from(end).unwrap_or(usize::MAX);
        let ids = conn
            .get(from..end)
            .ok_or_else(|| "cell offsets are not valid".to_owned())?;
        let point = |&i: &i64| {
            u32::try_from(i)
                .ok()
                .filter(|p| (*p as usize) < n_points)
                .ok_or_else(|| format!("a cell has the point {i}, not a point of the grid"))
        };
        grid.cells
            .push(ids.iter().map(point).collect::<Result<_, _>>()?);
        from = end;
    }
    if from != conn.len() {
        return err("the last cell offset does not end the connectivity");
    }
    grid.cell_types = types
        .iter()
        .map(|&t| u8::try_from(t).map_err(|_| format!("cell type {t} is not valid")))
        .collect::<Result<_, _>>()?;
    for h in headers.iter().filter(|h| h.section.ends_with("Data")) {
        let raw = block(data, h)?;
        let values = match h.type_name.as_str() {
            "Float64" => Values::F64(floats(h, &raw)?),
            "Int32" => Values::I32(ints(h, &raw)?.into_iter().map(|v| v as i32).collect()),
            t => return err(format!("array {}: unsupported type {t}", h.name)),
        };
        let n = if h.section == "PointData" {
            n_points
        } else {
            n_cells
        };
        if n.checked_mul(h.components) != Some(values.count()) {
            return err(format!(
                "array {} has {} values, not {n} x {}",
                h.name,
                values.count(),
                h.components
            ));
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

fn floats(h: &Header, raw: &[u8]) -> Result<Vec<f64>, String> {
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

/// Read a PVD collection file.
pub fn read_pvd(text: &str) -> Result<Vec<PvdEntry>, String> {
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
