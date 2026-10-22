//! The `.frd` result file of `ccx` as VTU grids: every nodal field in the file, for each step.
//!
//! The reader keeps the last increment of each step (and of each mode of a frequency step).
//! It renames the blocks `DISP`, `FORC`, `STRESS`, `TOSTRAIN`, `PE` and `NDTEMP` to the keywords
//! that a person writes (`U`, `RF`, `S`, `E`, `PEEQ`, `NT`) and adds `MISES` after each `S`. `ccx` writes
//! the element codes 1 to 12. The reader skips any other code. A file with no cell gives no frame.

use std::collections::HashMap;

use fh_core::vtu::{
    Field, Grid, VTK_HEXAHEDRON, VTK_LINE, VTK_QUAD, VTK_QUADRATIC_EDGE, VTK_QUADRATIC_HEXAHEDRON,
    VTK_QUADRATIC_QUAD, VTK_QUADRATIC_TETRA, VTK_QUADRATIC_TRIANGLE, VTK_QUADRATIC_WEDGE,
    VTK_TETRA, VTK_TRIANGLE, VTK_WEDGE,
};

/// The VTK cell type, the node count and the order of the nodes of the `.frd` element codes
/// that make a cell. `ccx` writes the nodes of a `C3D20` and of a `C3D15` in an order that
/// differs from the one of VTK: `order[i]` is the place in the `.frd` list of VTK node `i`.
fn cell_kind(code: u32) -> Option<(u8, &'static [usize])> {
    const HEX8: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
    const WEDGE6: [usize; 6] = [0, 1, 2, 3, 4, 5];
    const TET4: [usize; 4] = [0, 1, 2, 3];
    const TET10: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    const HEX20: [usize; 20] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 12, 13, 14, 15,
    ];
    const WEDGE15: [usize; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 9, 10, 11];
    Some(match code {
        1 => (VTK_HEXAHEDRON, &HEX8),
        2 => (VTK_WEDGE, &WEDGE6),
        3 => (VTK_TETRA, &TET4),
        4 => (VTK_QUADRATIC_HEXAHEDRON, &HEX20),
        5 => (VTK_QUADRATIC_WEDGE, &WEDGE15),
        6 => (VTK_QUADRATIC_TETRA, &TET10),
        7 => (VTK_TRIANGLE, &[0, 1, 2]),
        8 => (VTK_QUADRATIC_TRIANGLE, &[0, 1, 2, 3, 4, 5]),
        9 => (VTK_QUAD, &[0, 1, 2, 3]),
        10 => (VTK_QUADRATIC_QUAD, &[0, 1, 2, 3, 4, 5, 6, 7]),
        11 => (VTK_LINE, &[0, 1]),
        12 => (VTK_QUADRATIC_EDGE, &[0, 2, 1]),
        _ => return None,
    })
}

/// The name of a result block in the `.frd` file, and the name that the page shows.
fn field_name(block: &str) -> &str {
    match block {
        "DISP" => "U",
        "FORC" => "RF",
        "STRESS" => "S",
        "TOSTRAIN" => "E",
        "PE" => "PEEQ",
        "NDTEMP" => "NT",
        other => other,
    }
}

/// The results of one step, or of one mode.
#[derive(Debug, Default)]
pub struct Frame {
    /// The time of the increment, or the frequency of the mode, from the `.frd` file.
    pub time: f64,
    step: usize,
    increment: usize,
    mode: usize,
    fields: Vec<Field>,
}

/// The mesh and the frames of a `.frd` file.
#[derive(Debug, Default)]
pub struct Frd {
    points: Vec<[f64; 3]>,
    cell_types: Vec<u8>,
    cells: Vec<Vec<u32>>,
    /// The element number of each cell.
    pub elements: Vec<u32>,
    pub frames: Vec<Frame>,
}

impl Frd {
    /// The grid of a frame: the mesh of the file with the fields of the frame.
    pub fn grid(&self, frame: &Frame) -> Grid {
        Grid {
            points: self.points.clone(),
            cell_types: self.cell_types.clone(),
            cells: self.cells.clone(),
            point_data: frame.fields.clone(),
            cell_data: Vec::new(),
        }
    }
}

/// A block that the reader is inside.
enum Block {
    None,
    Nodes,
    Elements,
    /// A result block: its name, its type, and the rows read.
    Result(String, bool, Vec<(u32, Vec<f64>)>),
}

/// Read a number of `width` characters at `at`. A short line gives `None`.
fn cell(line: &str, at: usize, width: usize) -> Option<&str> {
    line.get(at..(at + width).min(line.len()))
        .filter(|s| !s.is_empty())
}

/// Read the ASCII `.frd` text.
pub fn read(text: &str) -> Result<Frd, String> {
    let bad = |n: usize, why: &str| format!("job.frd line {}: {why}", n + 1);
    let mut frd = Frd::default();
    let mut index: HashMap<u32, u32> = HashMap::new();
    // The number, the code and the nodes of each element.
    let mut elements: Vec<(u32, u32, Vec<u32>)> = Vec::new();
    let mut block = Block::None;
    let (mut step, mut increment, mut mode) = (0, 0, 0);
    let mut current: Option<usize> = None;
    let mut ended = false;
    for (n, line) in text.lines().enumerate() {
        if line.starts_with("    2C") {
            block = Block::Nodes;
        } else if line.starts_with("    3C") {
            block = Block::Elements;
        } else if line.starts_with("    1PSTEP") {
            let w: Vec<usize> = line
                .split_whitespace()
                .skip(1)
                .map_while(|w| w.parse().ok())
                .collect();
            let [_, inc, st] = w[..] else {
                return Err(bad(n, "bad PSTEP line"));
            };
            (increment, step, mode) = (inc, st, 0);
        } else if line.starts_with("    1PMODE") {
            mode = (line.split_whitespace().nth(1))
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| bad(n, "bad PMODE line"))?;
        } else if line.starts_with("  100CL") {
            let time = cell(line, 12, 12).and_then(|s| s.trim().parse().ok());
            let time = time.ok_or_else(|| bad(n, "bad time"))?;
            // A later increment of a step replaces the earlier one.
            let same = |f: &Frame| f.step == step && f.mode == mode;
            let at = match frd.frames.iter().position(same) {
                Some(at) => at,
                None => {
                    frd.frames.push(Frame {
                        step,
                        mode,
                        ..Frame::default()
                    });
                    frd.frames.len() - 1
                }
            };
            let frame = &mut frd.frames[at];
            if frame.increment != increment {
                frame.fields.clear();
            }
            (frame.time, frame.increment) = (time, increment);
            current = Some(at);
        } else if line.starts_with(" -4") {
            let name = cell(line, 5, 8).map_or("", str::trim).to_owned();
            // The type 1 is a value for each node. Other types hold a value for each element.
            let nodal = line.split_whitespace().last() == Some("1");
            block = Block::Result(name, nodal, Vec::new());
        } else if line.starts_with(" -3") {
            match std::mem::replace(&mut block, Block::None) {
                Block::Elements => {
                    for (number, code, ids) in elements.drain(..) {
                        let Some((kind, order)) = cell_kind(code) else {
                            continue;
                        };
                        if ids.len() != order.len() {
                            return Err(bad(n, "an element has a wrong count of nodes"));
                        }
                        let nodes = order.iter().map(|&i| index.get(&ids[i]).copied());
                        let cell: Option<Vec<u32>> = nodes.collect();
                        frd.cells
                            .push(cell.ok_or_else(|| bad(n, "an element has an unknown node"))?);
                        frd.cell_types.push(kind);
                        frd.elements.push(number);
                    }
                }
                Block::Result(name, true, rows) => {
                    let Some(at) = current else {
                        continue;
                    };
                    let k = rows.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
                    let mut values = vec![0.0; k * frd.points.len()];
                    for (id, row) in rows {
                        if let Some(&i) = index.get(&id) {
                            values[i as usize * k..][..row.len()].copy_from_slice(&row);
                        }
                    }
                    let name = field_name(&name);
                    let frame = &mut frd.frames[at];
                    frame
                        .fields
                        .retain(|f| f.name != name && !(name == "S" && f.name == "MISES"));
                    if name == "S" && k == 6 {
                        let mises = values.chunks(6).map(mises).collect();
                        frame.fields.push(Field::f64("MISES", 1, mises));
                    }
                    frame.fields.push(Field::f64(name, k, values));
                }
                _ => {}
            }
        } else if line.starts_with(" 9999") {
            ended = true;
        } else if line.starts_with(" -1") {
            match &mut block {
                Block::Nodes => {
                    let id = cell(line, 3, 10).and_then(|s| s.trim().parse().ok());
                    let xyz: Option<Vec<f64>> = (0..3)
                        .map(|k| cell(line, 13 + 12 * k, 12)?.trim().parse().ok())
                        .collect();
                    let (Some(id), Some(xyz)) = (id, xyz) else {
                        return Err(bad(n, "bad node line"));
                    };
                    index.insert(id, frd.points.len() as u32);
                    frd.points.push([xyz[0], xyz[1], xyz[2]]);
                }
                Block::Elements => {
                    let number = cell(line, 3, 10).and_then(|s| s.trim().parse().ok());
                    let code = cell(line, 13, 5).and_then(|s| s.trim().parse().ok());
                    let (Some(number), Some(code)) = (number, code) else {
                        return Err(bad(n, "bad element line"));
                    };
                    elements.push((number, code, Vec::new()));
                }
                Block::Result(_, true, rows) => {
                    let id = cell(line, 3, 10).and_then(|s| s.trim().parse().ok());
                    let id = id.ok_or_else(|| bad(n, "bad value line"))?;
                    rows.push((id, values(line, n)?));
                }
                _ => {}
            }
        } else if line.starts_with(" -2") {
            match &mut block {
                Block::Elements => {
                    let ids = (3..line.len())
                        .step_by(10)
                        .map(|i| cell(line, i, 10)?.trim().parse().ok());
                    let ids: Option<Vec<u32>> = ids.collect();
                    let ids = ids.ok_or_else(|| bad(n, "bad element node line"))?;
                    elements
                        .last_mut()
                        .ok_or_else(|| bad(n, "node line before an element"))?
                        .2
                        .extend(ids);
                }
                Block::Result(_, true, rows) => {
                    let more = values(line, n)?;
                    rows.last_mut()
                        .ok_or_else(|| bad(n, "value line before a node"))?
                        .1
                        .extend(more);
                }
                _ => {}
            }
        }
    }
    if !ended {
        return Err("job.frd has no end line 9999".into());
    }
    if frd.cells.is_empty() {
        frd.frames.clear();
    }
    Ok(frd)
}

/// The values of a data line: 12 characters each, after the first 3 columns and the id of
/// 10 characters on a ` -1` line.
fn values(line: &str, n: usize) -> Result<Vec<f64>, String> {
    let start = if line.starts_with(" -1") { 13 } else { 3 };
    (start..line.len())
        .step_by(12)
        .map(|i| {
            cell(line, i, 12)
                .and_then(|s| s.trim().parse().ok())
                .ok_or_else(|| format!("job.frd line {}: bad value", n + 1))
        })
        .collect()
}

/// The von Mises stress of `[xx, yy, zz, xy, yz, zx]`.
fn mises(s: &[f64]) -> f64 {
    let d = (s[0] - s[1]).powi(2) + (s[1] - s[2]).powi(2) + (s[2] - s[0]).powi(2);
    (0.5 * d + 3.0 * (s[3] * s[3] + s[4] * s[4] + s[5] * s[5])).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, x: f64) -> String {
        format!(" -1{id:10}{x:12.5E}{:12.5E}{:12.5E}\n", 0.0, 0.0)
    }

    #[test]
    fn a_later_increment_replaces_the_earlier_one_and_s_gets_mises() {
        let mut text = String::from(
            "    2C                             1                                     1\n",
        );
        text += &node(7, 1.0);
        text += " -3\n    3C                             1                                     1\n -1         1    3    0    1\n";
        text += &format!(
            " -2{}\n -3\n",
            [7, 7, 7, 7].map(|i| format!("{i:10}")).concat()
        );
        for (inc, v) in [(1, 1.0), (2, 2.0)] {
            text += &format!(
                "    1PSTEP{:25}{inc:12}{:12}\n  100CL  1{inc:02}{inc:12.5E}\n",
                inc, 1
            );
            text += " -4  STRESS      6    1\n";
            text += &format!(
                " -1{:10}{}\n -3\n",
                7,
                [v, 0.0, 0.0, 0.0, 0.0, 0.0]
                    .map(|x| format!("{x:12.5E}"))
                    .concat()
            );
        }
        text += " 9999\n";
        let frd = read(&text).unwrap();
        assert_eq!(frd.elements, [1]);
        assert_eq!(frd.frames.len(), 1);
        let grid = frd.grid(&frd.frames[0]);
        assert_eq!(frd.frames[0].time, 2.0);
        let s = grid.point_field("S").unwrap();
        assert_eq!((s.components, s.values[0]), (6, 2.0));
        assert_eq!(grid.point_field("MISES").unwrap().values, [2.0]);
    }
}
