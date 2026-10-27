//! The reader of a transmission line case: a `grid.toml` with a `[line]` table (`fh guide
//! grid`), and the text results and the result grid of its solution. `fh run` and the browser
//! solve of the viewer use the same code.

use fh_fem::grid::Part;
use fh_fem::line::{Conduction, Material, Model, Solution};
use fh_fem::mtl;
use toml_edit::{Document, TableLike};

use crate::grid::{footprint, named, number, numbers, only_in, optional, string, strings, table};
use crate::vtu::{Field, Grid, VTK_TRIANGLE};

/// The text of an error about an input out of the class of the line solver.
const CLASS: &str = "The built-in line solver gives the quasi-TEM per-length R, L, G and C and \
    the S parameters of a transmission line from its cross-section in x and y, with linear \
    materials. For other electromagnetic problems (a 3D field, a coil, an antenna, a field in \
    time, a nonlinear material), use GetDP (fh guide electromagnetics)";

/// The model of a line case. Lengths are in m: `units` must be SI-m.
pub fn read(text: &str) -> Result<Model, String> {
    let doc = Document::parse(text).map_err(|e| e.message().to_owned())?;
    let top: &dyn TableLike = doc.as_table();
    only_in(
        top,
        "the top",
        &["units", "grid", "material", "part", "line"],
        CLASS,
    )?;
    let units = string(top, "units", "the top")?;
    if units != "SI-m" {
        return Err(format!(
            "`units` of a line case must be SI-m (lengths in m, conductivity in S/m), not {units}"
        ));
    }
    let grid = table(top, "grid")?;
    only_in(grid, "[grid]", &["max_cell", "max_growth"], CLASS)?;
    let max_cell = number(grid, "max_cell", "[grid]")?;
    let max_growth = optional(grid, "max_growth", "[grid]")?.unwrap_or(1.3);
    let mut materials = Vec::new();
    for (name, item) in named(top, "material")? {
        let where_ = format!("[material.{name}]");
        only_in(
            item,
            &where_,
            &[
                "relative_permittivity",
                "loss_tangent",
                "relative_permeability",
                "electric_conductivity",
                "perfect_conductor",
                "roughness",
            ],
            CLASS,
        )?;
        let perfect = match item.get("perfect_conductor") {
            None => false,
            Some(v) => v
                .as_bool()
                .ok_or_else(|| format!("{where_}: `perfect_conductor` must be true or false"))?,
        };
        let sigma = optional(item, "electric_conductivity", &where_)?;
        let conduction = match (perfect, sigma) {
            (true, Some(_)) => {
                return Err(format!(
                    "{where_}: give electric_conductivity or perfect_conductor, not both"
                ))
            }
            (true, None) => Conduction::Perfect,
            (false, Some(s)) => Conduction::Finite(s),
            (false, None) => Conduction::None,
        };
        let roughness = optional(item, "roughness", &where_)?.unwrap_or(0.0);
        if roughness > 0.0 && !matches!(conduction, Conduction::Finite(_)) {
            return Err(format!(
                "{where_}: `roughness` needs electric_conductivity (a perfect conductor has no \
                 loss)"
            ));
        }
        materials.push(Material {
            name: name.to_owned(),
            permittivity: optional(item, "relative_permittivity", &where_)?.unwrap_or(1.0),
            loss_tangent: optional(item, "loss_tangent", &where_)?.unwrap_or(0.0),
            permeability: optional(item, "relative_permeability", &where_)?.unwrap_or(1.0),
            conduction,
            roughness,
        });
    }
    let mut parts = Vec::new();
    for (name, item) in named(top, "part")? {
        let where_ = format!("[part.{name}]");
        only_in(
            item,
            &where_,
            &[
                "material", "x", "y", "priority", "polygon", "circle", "holes",
            ],
            CLASS,
        )?;
        let m = string(item, "material", &where_)?;
        let material = materials
            .iter()
            .position(|x: &Material| x.name == m)
            .ok_or_else(|| format!("{where_}: no material {m}"))?;
        let footprint = footprint(item, &where_, 2)?;
        let mut lo = [0.0, 0.0, 0.0];
        let mut hi = [0.0, 0.0, 1.0];
        for (d, axis) in ["x", "y"].iter().enumerate() {
            if footprint.is_some() {
                if item.contains_key(axis) {
                    return Err(format!(
                        "{where_}: a part with a polygon or a circle has no range `{axis}`"
                    ));
                }
                continue;
            }
            let range = item
                .get(axis)
                .and_then(numbers)
                .filter(|v| v.len() == 2)
                .ok_or_else(|| {
                    format!("{where_}: `{axis}` must be 2 numbers, the range of the rectangle")
                })?;
            lo[d] = range[0];
            hi[d] = range[1];
        }
        let priority = match item.get("priority") {
            None => 0,
            Some(p) => p
                .as_integer()
                .ok_or(format!("{where_}: `priority` must be a whole number"))?,
        };
        parts.push(Part {
            name: name.to_owned(),
            material,
            priority,
            lo,
            hi,
            footprint,
        });
    }
    let line = table(top, "line")?;
    only_in(
        line,
        "[line]",
        &[
            "signals",
            "references",
            "electric_walls",
            "frequencies",
            "length",
            "reference_impedance",
        ],
        CLASS,
    )?;
    let part_list = |key: &str| -> Result<Vec<usize>, String> {
        match line.get(key) {
            None => Ok(Vec::new()),
            Some(item) => strings(item, &format!("[line] {key}"))?
                .iter()
                .map(|p| {
                    parts
                        .iter()
                        .position(|x: &Part| x.name == *p)
                        .ok_or_else(|| format!("[line] {key}: no part {p}"))
                })
                .collect(),
        }
    };
    let signals = part_list("signals")?;
    let references = part_list("references")?;
    let mut electric_walls = [false; 4];
    if let Some(item) = line.get("electric_walls") {
        for side in strings(item, "[line] electric_walls")? {
            let k = ["x-", "x+", "y-", "y+"]
                .iter()
                .position(|s| *s == side)
                .ok_or_else(|| {
                    format!("[line] electric_walls: `{side}` must be x-, x+, y- or y+")
                })?;
            electric_walls[k] = true;
        }
    }
    let frequencies = line
        .get("frequencies")
        .and_then(|f| numbers(f).or_else(|| crate::grid::as_number(f).map(|v| vec![v])))
        .ok_or("[line]: `frequencies` must be a number or a list of numbers, in Hz")?;
    Ok(Model {
        parts,
        materials,
        signals,
        references,
        electric_walls,
        frequencies,
        max_cell,
        max_growth,
        max_points: LAPTOP_POINTS,
        length: optional(line, "length", "[line]")?,
        reference_impedance: optional(line, "reference_impedance", "[line]")?.unwrap_or(50.0),
    })
}

/// The largest mesh of `fh run`. Reason: the 2D direct solve of 321,295 points (2 conductors at
/// 1 GHz) took 0.7 s for each frequency and a factor of 12.7 million entries (200 MB complex), so
/// 1,000,000 points fit in the memory of a laptop.
pub const LAPTOP_POINTS: usize = 1_000_000;

/// The table of the per-length parameters: 1 row for each frequency. The columns are `f` (Hz),
/// `R{i}_{j}` (ohm/m), `L{i}_{j}` (H/m), `G{i}_{j}` (S/m) and `C{i}_{j}` (F/m) for the signals i
/// and j from 1, then `eps_eff{m}` and `loss{m}` (dB/m) of each mode m, the largest effective
/// permittivity first, and `z0_re z0_im` (ohm) for 1 signal.
pub fn table_text(model: &Model, solution: &Solution) -> String {
    let n = model.signals.len();
    let mut out = String::new();
    for (i, &p) in model.signals.iter().enumerate() {
        out.push_str(&format!(
            "# signal {}: part {}\n",
            i + 1,
            model.parts[p].name
        ));
    }
    let mut head = vec!["f".to_owned()];
    for q in ["R", "L", "G", "C"] {
        for i in 1..=n {
            for j in 1..=n {
                head.push(format!("{q}{i}_{j}"));
            }
        }
    }
    for m in 1..=n {
        head.push(format!("eps_eff{m}"));
        head.push(format!("loss{m}"));
    }
    if n == 1 {
        head.push("z0_re".into());
        head.push("z0_im".into());
    }
    out.push_str(&format!("# {}\n", head.join(" ")));
    for p in &solution.params {
        let mut row = vec![p.frequency];
        for m in [&p.r, &p.l, &p.g, &p.c] {
            row.extend_from_slice(m);
        }
        for mode in &p.modes {
            row.push(mode.eps_eff);
            row.push(mode.loss_db);
        }
        if let Some(z) = p.z0 {
            row.push(z.re);
            row.push(z.im);
        }
        let words: Vec<String> = row.iter().map(|v| format!("{:.10e}", v + 0.0)).collect();
        out.push_str(&words.join(" "));
        out.push('\n');
    }
    out
}

/// The name of the Touchstone file of a line of N signals: `line.s{2N}p`.
pub fn touchstone_name(model: &Model) -> String {
    format!("line.s{}p", 2 * model.signals.len())
}

/// The Touchstone text of the S matrices, or `None` with no `length`.
pub fn touchstone_text(model: &Model, solution: &Solution) -> Option<String> {
    let length = model.length?;
    let n = model.signals.len();
    let mut comments = vec![format!(
        "femahoy line solver: {length} m of line, reference impedance {} ohm",
        model.reference_impedance
    )];
    for (i, &p) in model.signals.iter().enumerate() {
        comments.push(format!(
            "port {} and port {}: the near and the far end of part {}",
            i + 1,
            n + i + 1,
            model.parts[p].name
        ));
    }
    let f: Vec<f64> = solution.params.iter().map(|p| p.frequency).collect();
    let s: Vec<_> = solution
        .params
        .iter()
        .map(|p| p.s.clone().expect("an S matrix with a length"))
        .collect();
    Some(mtl::touchstone(
        &f,
        &s,
        model.reference_impedance,
        &comments,
    ))
}

/// The result grid: the triangles at z = 0, the potential `V{j}` (V) at each point with signal j
/// at 1 V, and the amplitude of the current density `J{j}` (A/m2) with 1 A in signal j at the
/// highest frequency and the part number `part` (from 1) of each triangle.
pub fn solution_grid(solution: &Solution) -> Grid {
    let mut point_data = Vec::new();
    for (j, v) in solution.potential.iter().enumerate() {
        point_data.push(Field::f64(&format!("V{}", j + 1), 1, v.clone()));
    }
    let mut cell_data = Vec::new();
    for (j, c) in solution.current.iter().enumerate() {
        cell_data.push(Field::f64(&format!("J{}", j + 1), 1, c.clone()));
    }
    cell_data.push(Field::f64(
        "part",
        1,
        solution.part.iter().map(|&p| f64::from(p) + 1.0).collect(),
    ));
    Grid {
        points: solution.points.iter().map(|p| [p[0], p[1], 0.0]).collect(),
        cell_types: vec![VTK_TRIANGLE; solution.triangles.len()],
        cells: solution.triangles.iter().map(|t| t.to_vec()).collect(),
        point_data,
        cell_data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: &str = r#"
units = "SI-m"
[grid]
max_cell = 2e-4
[material.air]
[material.substrate]
relative_permittivity = 4.0
loss_tangent = 0.01
[material.copper]
electric_conductivity = 5.8e7
roughness = 1e-6
[part.air]
material = "air"
x = [-5e-3, 5e-3]
y = [0.0, 5e-3]
[part.substrate]
material = "substrate"
priority = 1
x = [-5e-3, 5e-3]
y = [0.0, 1e-3]
[part.strip]
material = "copper"
priority = 2
x = [-1e-3, 1e-3]
y = [1e-3, 1.035e-3]
[line]
signals = "strip"
electric_walls = ["y-"]
frequencies = [1e8, 1e9]
length = 0.1
"#;

    #[test]
    fn the_reader_reads_a_line_and_refuses_keys_out_of_its_class() {
        assert!(crate::grid::is_line(STRIP));
        let m = read(STRIP).unwrap();
        assert_eq!(
            (m.parts.len(), m.signals, m.frequencies.len()),
            (3, vec![2], 2)
        );
        assert_eq!(m.electric_walls, [false, false, true, false]);
        assert_eq!(m.materials[2].conduction, Conduction::Finite(5.8e7));
        assert_eq!(m.length, Some(0.1));
        for (text, key) in [
            (STRIP.replace("[line]", "[line]\nmode = \"3d\""), "mode"),
            (STRIP.replace("[grid]", "[grid]\naxis = \"z\""), "axis"),
            (
                STRIP.replace("loss_tangent = 0.01", "loss_tangent = 0.01\nnonlinear = 1"),
                "nonlinear",
            ),
            (
                STRIP.replace("units = \"SI-m\"", "units = \"SI-m\"\n[step]"),
                "step",
            ),
        ] {
            let e = read(&text).unwrap_err();
            assert!(e.contains(key) && e.contains("GetDP"), "{e}");
        }
        let e = read(&STRIP.replace("SI-m", "SI-mm")).unwrap_err();
        assert!(e.contains("must be SI-m"), "{e}");
    }

    /// The table has a row for each frequency and the Touchstone file 2 ports.
    #[test]
    fn a_microstrip_gives_the_table_and_the_touchstone_text() {
        struct Quiet;
        impl fh_fem::line::Events for Quiet {
            fn progress(&mut self, _: &str) {}
        }
        let m = read(STRIP).unwrap();
        let s = fh_fem::line::solve(&m, &mut Quiet).unwrap();
        let t = table_text(&m, &s);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "# signal 1: part strip");
        assert!(lines[1].starts_with("# f R1_1 L1_1 G1_1 C1_1 eps_eff1 loss1 z0_re z0_im"));
        assert_eq!(lines.len(), 4);
        assert_eq!(touchstone_name(&m), "line.s2p");
        let ts = touchstone_text(&m, &s).unwrap();
        assert!(ts.contains("# Hz S RI R 50"));
        assert_eq!(ts.lines().filter(|l| !l.starts_with(['!', '#'])).count(), 2);
        let g = solution_grid(&s);
        assert_eq!(g.cell_types.len(), s.triangles.len());
        assert!(g.point_field("V1").is_some());
    }
}
