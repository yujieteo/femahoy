//! Run results to view data: the steps of a `results.pvd`, the displacement of each step,
//! the contour fields, the deformed shape and the contour bands.
//!
//! Plain functions with no GPU or file types. The caller reads the files (from the disk in
//! the native app, over HTTP in the browser) and gives the bytes to `load`.

use std::fmt;

use fh_io::vtu::{read_pvd, read_vtu, Grid};

use crate::mesh::{self, RenderMesh};

/// The number of colour bands in a contour plot.
pub const BANDS: usize = 9;

/// The band colours from the lowest value to the highest (blue, cyan, green, yellow, red).
pub const BAND_COLORS: [[f32; 3]; BANDS] = [
    [0.00, 0.00, 1.00],
    [0.00, 0.40, 1.00],
    [0.00, 0.80, 1.00],
    [0.00, 1.00, 0.60],
    [0.20, 1.00, 0.00],
    [0.70, 1.00, 0.00],
    [1.00, 0.85, 0.00],
    [1.00, 0.45, 0.00],
    [1.00, 0.00, 0.00],
];

/// The deformed shape is drawn so that the largest displacement is this part of the model
/// size, when the automatic scale is on.
pub const AUTO_SCALE_FRACTION: f64 = 0.1;

/// A problem with the result files. `file` names the file that has the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub code: &'static str,
    pub file: String,
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {} ({})", self.file, self.message, self.code)
    }
}

impl std::error::Error for LoadError {}

fn fail<T>(code: &'static str, file: &str, message: impl Into<String>) -> Result<T, LoadError> {
    Err(LoadError {
        code,
        file: file.to_owned(),
        message: message.into(),
    })
}

/// Where the values of a field are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    /// One value at each node.
    Point,
    /// One value for each element, constant over the element.
    Cell,
}

/// One scalar field that the contour can show.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarField {
    /// For example `MISES`, `U magnitude` or `S11`.
    pub name: String,
    /// The quantity, for the unit: `displacement`, `force` or `stress`.
    pub quantity: Quantity,
    pub location: Location,
    pub values: Vec<f64>,
}

/// The physical quantity of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    Displacement,
    Force,
    Stress,
    Other,
}

/// The results of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The step time at the end of the step, from `results.pvd`.
    pub time: f64,
    /// The VTU file name, from `results.pvd`.
    pub file: String,
    /// The displacement `U` of each node, in model order.
    pub displacement: Vec<[f64; 3]>,
    pub fields: Vec<ScalarField>,
}

impl Step {
    pub fn field(&self, name: &str) -> Option<&ScalarField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// The largest displacement magnitude in the step.
    pub fn max_displacement(&self) -> f64 {
        self.displacement
            .iter()
            .map(|u| (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt())
            .fold(0.0, f64::max)
    }
}

/// All the steps of one run on one mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// The unit system of the run (`SI-mm`, `SI-m` or `unspecified`).
    pub units: String,
    pub mesh: RenderMesh,
    pub steps: Vec<Step>,
}

/// The `units` attribute of the `Collection` element, or `unspecified` when it is absent.
fn pvd_units(text: &str) -> String {
    text.split_once("<Collection")
        .and_then(|(_, rest)| rest.split_once('>'))
        .and_then(|(attrs, _)| attrs.split_once("units=\""))
        .and_then(|(_, v)| v.split_once('"'))
        .map_or_else(|| "unspecified".to_owned(), |(v, _)| v.to_owned())
}

/// The names of the components of a field with `components` values per item.
fn component_names(name: &str, components: usize) -> Vec<String> {
    match (name, components) {
        ("S", 6) => ["S11", "S22", "S33", "S12", "S23", "S13"]
            .map(String::from)
            .to_vec(),
        (_, 3) => ["1", "2", "3"].map(|c| format!("{name}{c}")).to_vec(),
        (_, 1) => vec![name.to_owned()],
        _ => (1..=components).map(|c| format!("{name}{c}")).collect(),
    }
}

fn quantity(name: &str) -> Quantity {
    match name {
        "U" => Quantity::Displacement,
        "RF" => Quantity::Force,
        "S" | "MISES" => Quantity::Stress,
        _ => Quantity::Other,
    }
}

/// The contour fields of a grid: the magnitude and the components of each vector field,
/// the components of `S`, and each scalar field. Integer arrays (ids and sets) are not fields.
fn fields_of(grid: &Grid, file: &str) -> Result<Vec<ScalarField>, LoadError> {
    let mut out = Vec::new();
    let groups = [
        (Location::Point, &grid.point_data, grid.points.len()),
        (Location::Cell, &grid.cell_data, grid.cells.len()),
    ];
    for (location, data, count) in groups {
        for field in data.iter() {
            let Some(values) = field.as_f64() else {
                continue;
            };
            let c = field.components.max(1);
            if values.len() != c * count {
                return fail(
                    "results.field.length",
                    file,
                    format!(
                        "field {} has {} values, expected {} ({} x {c})",
                        field.name,
                        values.len(),
                        c * count,
                        count
                    ),
                );
            }
            if let Some(i) = values.iter().position(|v| !v.is_finite()) {
                return fail(
                    "results.field.nonfinite",
                    file,
                    format!(
                        "field {} has value {} at item {}",
                        field.name,
                        values[i],
                        i / c
                    ),
                );
            }
            let q = quantity(&field.name);
            if c == 3 {
                out.push(ScalarField {
                    name: format!("{} magnitude", field.name),
                    quantity: q,
                    location,
                    values: values
                        .chunks_exact(3)
                        .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
                        .collect(),
                });
            }
            for (k, name) in component_names(&field.name, c).into_iter().enumerate() {
                out.push(ScalarField {
                    name,
                    quantity: q,
                    location,
                    values: values.iter().skip(k).step_by(c).copied().collect(),
                });
            }
        }
    }
    Ok(out)
}

/// Read the results of a run. `pvd` is the text of `results.pvd`. `read` gives the bytes of a
/// file that `results.pvd` names. Each step must have the mesh of the first step and a
/// displacement field `U`.
pub fn load(
    pvd_name: &str,
    pvd: &str,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Results, LoadError> {
    let entries = match read_pvd(pvd) {
        Ok(e) => e,
        Err(e) => return fail(e.code, pvd_name, e.message),
    };
    if entries.is_empty() {
        return fail(
            "results.empty",
            pvd_name,
            "the collection names no step file",
        );
    }
    let mut first: Option<Grid> = None;
    let mut steps = Vec::with_capacity(entries.len());
    for entry in entries {
        let file = entry.file.as_str();
        if file.contains("..") || file.starts_with('/') || file.contains('\\') {
            return fail(
                "results.file.path",
                pvd_name,
                format!("step file {file} is not in the run folder"),
            );
        }
        let bytes = match read(file) {
            Ok(b) => b,
            Err(e) => return fail("results.file.missing", file, e),
        };
        let grid = match read_vtu(&bytes) {
            Ok(g) => g,
            Err(e) => return fail(e.code, file, e.message),
        };
        if grid.points.is_empty() || grid.cells.is_empty() {
            return fail(
                "results.mesh.empty",
                file,
                "the grid has no point or no cell",
            );
        }
        if let Some(f) = &first {
            if f.points != grid.points || f.cells != grid.cells || f.cell_types != grid.cell_types {
                return fail(
                    "results.mesh.changed",
                    file,
                    format!("the mesh is not the mesh of {}", steps_file(&steps)),
                );
            }
        }
        let fields = fields_of(&grid, file)?;
        let Some(u) = grid.point_field("U") else {
            return fail("results.field.missing", file, "no displacement field U");
        };
        let (Some(u), 3) = (u.as_f64(), u.components) else {
            return fail(
                "results.field.missing",
                file,
                "the displacement field U must have 3 float components",
            );
        };
        steps.push(Step {
            time: entry.time,
            file: entry.file.clone(),
            displacement: u.chunks_exact(3).map(|v| [v[0], v[1], v[2]]).collect(),
            fields,
        });
        if first.is_none() {
            first = Some(grid);
        }
    }
    let grid = first.expect("one step at least");
    let mesh = mesh::build_grid(&grid);
    if mesh.triangles.is_empty() {
        return fail(
            "results.mesh.unsupported",
            &steps[0].file,
            format!(
                "no C3D4, C3D8 or C3D10 cell to draw ({} cells)",
                grid.cells.len()
            ),
        );
    }
    Ok(Results {
        units: pvd_units(pvd),
        mesh,
        steps,
    })
}

fn steps_file(steps: &[Step]) -> &str {
    steps.first().map_or("the first step", |s| s.file.as_str())
}

/// The unit of a quantity in a unit system, or an empty text when it is not known.
pub fn unit(units: &str, quantity: Quantity) -> &'static str {
    match (units, quantity) {
        ("SI-mm", Quantity::Displacement) => "mm",
        ("SI-mm", Quantity::Stress) => "MPa",
        ("SI-m", Quantity::Displacement) => "m",
        ("SI-m", Quantity::Stress) => "Pa",
        ("SI-mm" | "SI-m", Quantity::Force) => "N",
        _ => "",
    }
}

/// A scale factor near `raw` that is 1, 2 or 5 times a power of ten. Zero and values that
/// are not finite give 1.
pub fn nice_scale(raw: f64) -> f64 {
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let m = raw / power;
    let nice = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// The automatic scale factor: the largest displacement of all steps shows as
/// `AUTO_SCALE_FRACTION` of the model size. A model with no displacement gives 1.
pub fn auto_scale(results: &Results) -> f64 {
    let size = results
        .mesh
        .bounds
        .map_or(0.0, |b| 2.0 * f64::from(b.radius()));
    let max = results
        .steps
        .iter()
        .map(Step::max_displacement)
        .fold(0.0, f64::max);
    if max > 0.0 {
        nice_scale(AUTO_SCALE_FRACTION * size / max)
    } else {
        1.0
    }
}

/// The deformed position of each render position: `x + scale * U`.
pub fn deformed_positions(mesh: &RenderMesh, step: &Step, scale: f64) -> Vec<[f32; 3]> {
    mesh.positions
        .iter()
        .zip(&mesh.source_node)
        .map(|(p, &n)| {
            let u = step.displacement[n as usize];
            std::array::from_fn(|k| (f64::from(p[k]) + scale * u[k]) as f32)
        })
        .collect()
}

/// The smallest and the largest value of a field.
pub fn range(field: &ScalarField) -> (f64, f64) {
    field
        .values
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        })
}

/// A spread of values smaller than this part of their size is round-off: the field is
/// constant.
pub const CONSTANT_SPREAD: f64 = 1e-9;

/// The range that the contour shows: `range`, or one value (the middle of the range) when
/// the spread is round-off. A uniform stress state then shows as one band, not as 9 bands
/// of noise.
pub fn contour_range(field: &ScalarField) -> (f64, f64) {
    let (lo, hi) = range(field);
    if hi - lo <= CONSTANT_SPREAD * lo.abs().max(hi.abs()) {
        let mid = 0.5 * (lo + hi);
        (mid, mid)
    } else {
        (lo, hi)
    }
}

/// The value of a field at each corner of each surface triangle, three per triangle. A
/// point field gives the node value. A cell field gives the value of the owning element.
pub fn triangle_values(mesh: &RenderMesh, field: &ScalarField) -> Vec<f32> {
    let mut out = Vec::with_capacity(3 * mesh.triangles.len());
    for (t, cell) in mesh.triangles.iter().zip(&mesh.triangle_cell) {
        for &p in t {
            let v = match field.location {
                Location::Point => field.values[mesh.source_node[p as usize] as usize],
                Location::Cell => field.values[*cell as usize],
            };
            out.push(v as f32);
        }
    }
    out
}

/// The band (0 to `BANDS - 1`) of a value in the range `lo..=hi`. The shader uses the same
/// rule. A range of zero width gives the middle band.
pub fn band(value: f64, lo: f64, hi: f64) -> usize {
    if hi <= lo {
        return BANDS / 2;
    }
    let t = (value - lo) / (hi - lo);
    ((t * BANDS as f64).floor().max(0.0) as usize).min(BANDS - 1)
}

/// The band limits: `BANDS + 1` values from `lo` to `hi`.
pub fn band_limits(lo: f64, hi: f64) -> Vec<f64> {
    (0..=BANDS)
        .map(|i| lo + (hi - lo) * i as f64 / BANDS as f64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    use fh_io::vtu::{write_vtu, Field};

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/results/cantilever-2step")
    }

    fn load_dir(dir: &std::path::Path) -> Result<Results, LoadError> {
        let pvd = std::fs::read_to_string(dir.join("results.pvd")).unwrap();
        load("results.pvd", &pvd, |f| {
            std::fs::read(dir.join(f)).map_err(|e| e.to_string())
        })
    }

    /// The text results of one step: node id to (U, RF), element id to (S, MISES).
    struct Text {
        nodes: HashMap<i32, [f64; 6]>,
        elements: HashMap<i32, [f64; 7]>,
    }

    fn read_text(path: PathBuf) -> Text {
        let text = std::fs::read_to_string(path).unwrap();
        let mut nodes = HashMap::new();
        let mut elements = HashMap::new();
        let mut block = "";
        for line in text.lines() {
            if line.starts_with("node_id") {
                block = "nodes";
                continue;
            }
            if line.starts_with("element_id S11") {
                block = "elements";
                continue;
            }
            let cols: Vec<&str> = line.split_whitespace().collect();
            let num = |s: &str| s.parse::<f64>().unwrap();
            match (block, cols.len()) {
                ("nodes", 7) => {
                    nodes.insert(
                        cols[0].parse().unwrap(),
                        std::array::from_fn(|k| num(cols[k + 1])),
                    );
                }
                ("elements", 8) => {
                    elements.insert(
                        cols[0].parse().unwrap(),
                        std::array::from_fn(|k| num(cols[k + 1])),
                    );
                }
                _ => block = if cols.is_empty() { "" } else { block },
            }
        }
        Text { nodes, elements }
    }

    fn ids(dir: &std::path::Path, file: &str, name: &str, point: bool) -> Vec<i32> {
        let grid = read_vtu(&std::fs::read(dir.join(file)).unwrap()).unwrap();
        let f = if point {
            grid.point_field(name)
        } else {
            grid.cell_field(name)
        };
        match &f.unwrap().values {
            fh_io::vtu::Values::I32(v) => v.clone(),
            _ => panic!("ids are integers"),
        }
    }

    #[test]
    fn fixture_has_two_steps_with_units_and_times() {
        let r = load_dir(&fixture_dir()).unwrap();
        assert_eq!(r.units, "SI-mm");
        assert_eq!(r.steps.len(), 2);
        assert_eq!((r.steps[0].time, r.steps[1].time), (1.0, 2.0));
        assert_eq!(r.steps[1].file, "step-2.vtu");
        assert_eq!((r.mesh.node_count, r.mesh.element_count), (20, 4));
        assert_eq!(r.mesh.skipped_elements, 0);
        let names: Vec<&str> = r.steps[0].fields.iter().map(|f| f.name.as_str()).collect();
        for want in [
            "U magnitude",
            "U1",
            "U3",
            "RF magnitude",
            "S11",
            "S13",
            "MISES",
        ] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
    }

    /// Each displacement and each contour value matches the text results of the same
    /// run, item by item.
    #[test]
    fn values_match_the_text_results() {
        let dir = fixture_dir();
        let r = load_dir(&dir).unwrap();
        for (i, step) in r.steps.iter().enumerate() {
            let text = read_text(dir.join(format!("step-{}.txt", i + 1)));
            let node_ids = ids(&dir, &step.file, "NODE_ID", true);
            let element_ids = ids(&dir, &step.file, "ELEMENT_ID", false);
            assert_eq!(node_ids.len(), text.nodes.len());
            for (n, id) in node_ids.iter().enumerate() {
                let t = text.nodes[id];
                assert_eq!(step.displacement[n], [t[0], t[1], t[2]], "U node {id}");
                let mag = step.field("U magnitude").unwrap().values[n];
                let want = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
                assert!((mag - want).abs() <= 1e-15 * want.max(1.0));
            }
            let names = ["S11", "S22", "S33", "S12", "S23", "S13", "MISES"];
            for (e, id) in element_ids.iter().enumerate() {
                for (k, name) in names.iter().enumerate() {
                    let v = step.field(name).unwrap().values[e];
                    assert_eq!(v, text.elements[id][k], "{name} element {id}");
                }
            }
        }
        // The second step doubles the load of the linear model.
        let (a, b) = (&r.steps[0], &r.steps[1]);
        let ratio = b.max_displacement() / a.max_displacement();
        assert!((ratio - 2.0).abs() < 1e-9, "ratio {ratio}");
    }

    #[test]
    fn deformed_positions_add_the_scaled_displacement() {
        let r = load_dir(&fixture_dir()).unwrap();
        let step = &r.steps[1];
        let scale = 50.0;
        let moved = deformed_positions(&r.mesh, step, scale);
        assert_eq!(moved.len(), r.mesh.positions.len());
        for (i, p) in moved.iter().enumerate() {
            let u = step.displacement[r.mesh.source_node[i] as usize];
            for k in 0..3 {
                let want = f64::from(r.mesh.positions[i][k]) + scale * u[k];
                assert!((f64::from(p[k]) - want).abs() < 1e-5, "position {i}");
            }
        }
        // The tip (x = 10) moves down in z by 50 x the tip U3 of step 2.
        let tip = r.mesh.positions.iter().position(|p| p[0] == 10.0).unwrap();
        let u3 = step.displacement[r.mesh.source_node[tip] as usize][2];
        assert!((u3 - -0.03685902950909793).abs() < 1e-6);
        assert!((f64::from(moved[tip][2] - r.mesh.positions[tip][2]) - scale * u3).abs() < 1e-5);
        // Scale 0 is the undeformed shape.
        assert_eq!(deformed_positions(&r.mesh, step, 0.0), r.mesh.positions);
    }

    #[test]
    fn auto_scale_shows_the_largest_displacement_as_a_tenth_of_the_size() {
        let r = load_dir(&fixture_dir()).unwrap();
        let s = auto_scale(&r);
        let size = 2.0 * f64::from(r.mesh.bounds.unwrap().radius());
        let max = r.steps[1].max_displacement();
        let shown = s * max / size;
        // A nice scale is within a factor of 2 of the exact value.
        assert!((0.05..=0.2).contains(&shown), "scale {s}, shown {shown}");
        assert_eq!(nice_scale(1.2), 1.0);
        assert_eq!(nice_scale(27.0), 20.0);
        assert_eq!(nice_scale(0.04), 0.05);
        assert_eq!(nice_scale(80.0), 100.0);
        assert_eq!(nice_scale(0.0), 1.0);
        assert_eq!(nice_scale(f64::NAN), 1.0);
    }

    #[test]
    fn triangle_values_use_the_node_or_the_owning_element() {
        let r = load_dir(&fixture_dir()).unwrap();
        let step = &r.steps[0];
        let mises = step.field("MISES").unwrap();
        let vals = triangle_values(&r.mesh, mises);
        assert_eq!(vals.len(), 3 * r.mesh.triangles.len());
        for (t, cell) in r.mesh.triangle_cell.iter().enumerate() {
            for c in 0..3 {
                assert_eq!(vals[3 * t + c], mises.values[*cell as usize] as f32);
            }
        }
        let u3 = step.field("U3").unwrap();
        let vals = triangle_values(&r.mesh, u3);
        for (t, tri) in r.mesh.triangles.iter().enumerate() {
            for c in 0..3 {
                let node = r.mesh.source_node[tri[c] as usize] as usize;
                assert_eq!(vals[3 * t + c], u3.values[node] as f32);
            }
        }
    }

    #[test]
    fn bands_split_the_range_in_equal_parts() {
        assert_eq!(band(0.0, 0.0, 9.0), 0);
        assert_eq!(band(0.99, 0.0, 9.0), 0);
        assert_eq!(band(1.0, 0.0, 9.0), 1);
        assert_eq!(band(9.0, 0.0, 9.0), BANDS - 1);
        assert_eq!(band(-5.0, 0.0, 9.0), 0);
        assert_eq!(band(50.0, 0.0, 9.0), BANDS - 1);
        assert_eq!(band(3.0, 3.0, 3.0), BANDS / 2);
        let l = band_limits(-1.0, 8.0);
        assert_eq!(l.len(), BANDS + 1);
        assert_eq!((l[0], l[1], l[BANDS]), (-1.0, 0.0, 8.0));
        let f = ScalarField {
            name: "x".into(),
            quantity: Quantity::Other,
            location: Location::Point,
            values: vec![2.0, -1.0, 5.0],
        };
        assert_eq!(range(&f), (-1.0, 5.0));
    }

    /// A uniform stress with round-off noise, as in the bar-pressure case, is one value.
    #[test]
    fn a_range_within_round_off_is_constant() {
        let field = |values: Vec<f64>| ScalarField {
            name: "MISES".into(),
            quantity: Quantity::Stress,
            location: Location::Cell,
            values,
        };
        let noisy = field(vec![100.0 - 3e-11, 100.0, 100.0 + 2e-11]);
        let (lo, hi) = contour_range(&noisy);
        assert_eq!(lo, hi);
        assert!((lo - 100.0).abs() < 1e-10);
        let real = field(vec![99.0, 100.0]);
        assert_eq!(contour_range(&real), (99.0, 100.0));
        let zero = field(vec![0.0, 1e-300]);
        assert_eq!(contour_range(&zero), (0.0, 1e-300));
    }

    #[test]
    fn units_follow_the_unit_system() {
        assert_eq!(unit("SI-mm", Quantity::Stress), "MPa");
        assert_eq!(unit("SI-m", Quantity::Stress), "Pa");
        assert_eq!(unit("SI-mm", Quantity::Displacement), "mm");
        assert_eq!(unit("SI-m", Quantity::Force), "N");
        assert_eq!(unit("unspecified", Quantity::Stress), "");
        assert_eq!(pvd_units("<Collection>"), "unspecified");
    }

    // The error cases. Each one names the file and has a stable code.

    fn one_step(grid: &Grid) -> Result<Results, LoadError> {
        let pvd = fh_io::vtu::write_pvd_units(
            &[fh_io::vtu::PvdEntry {
                time: 1.0,
                file: "step-1.vtu".into(),
            }],
            "SI-mm",
        );
        let bytes = write_vtu(grid).unwrap();
        load("results.pvd", &pvd, |_| Ok(bytes.clone()))
    }

    fn fixture_grid() -> Grid {
        read_vtu(&std::fs::read(fixture_dir().join("step-1.vtu")).unwrap()).unwrap()
    }

    #[test]
    fn a_missing_step_file_names_the_file() {
        let pvd = std::fs::read_to_string(fixture_dir().join("results.pvd")).unwrap();
        let e = load("results.pvd", &pvd, |f| {
            if f == "step-2.vtu" {
                Err("not found".into())
            } else {
                std::fs::read(fixture_dir().join(f)).map_err(|e| e.to_string())
            }
        })
        .unwrap_err();
        assert_eq!(
            (e.code, e.file.as_str()),
            ("results.file.missing", "step-2.vtu")
        );
        assert_eq!(
            e.to_string(),
            "step-2.vtu: not found (results.file.missing)"
        );
    }

    #[test]
    fn a_bad_collection_or_an_empty_one_is_an_error() {
        let e = load("results.pvd", "hello", |_| Ok(Vec::new())).unwrap_err();
        assert_eq!(e.file, "results.pvd");
        assert_eq!(e.code, "VTU002");
        let empty = fh_io::vtu::write_pvd(&[]);
        let e = load("results.pvd", &empty, |_| Ok(Vec::new())).unwrap_err();
        assert_eq!(e.code, "results.empty");
    }

    #[test]
    fn a_step_file_outside_the_run_folder_is_refused() {
        let pvd = fh_io::vtu::write_pvd(&[fh_io::vtu::PvdEntry {
            time: 1.0,
            file: "../secret.vtu".into(),
        }]);
        let e = load("results.pvd", &pvd, |_| panic!("must not read")).unwrap_err();
        assert_eq!(e.code, "results.file.path");
    }

    #[test]
    fn a_corrupt_vtu_is_an_error() {
        let pvd = std::fs::read_to_string(fixture_dir().join("results.pvd")).unwrap();
        let e = load("results.pvd", &pvd, |_| Ok(b"<VTKFile>".to_vec())).unwrap_err();
        assert_eq!(e.file, "step-1.vtu");
        assert!(e.code.starts_with("VTU"), "{e}");
    }

    #[test]
    fn a_step_with_no_displacement_is_an_error() {
        let mut g = fixture_grid();
        g.point_data.retain(|f| f.name != "U");
        let e = one_step(&g).unwrap_err();
        assert_eq!(e.code, "results.field.missing");
        assert!(e.message.contains("U"));
    }

    #[test]
    fn a_value_that_is_not_finite_is_an_error() {
        let mut g = fixture_grid();
        let n = g.cells.len();
        g.cell_data.retain(|f| f.name != "MISES");
        let mut v = vec![1.0; n];
        v[2] = f64::NAN;
        g.cell_data.push(Field::f64("MISES", 1, v));
        let e = one_step(&g).unwrap_err();
        assert_eq!(e.code, "results.field.nonfinite");
        assert!(
            e.message.contains("MISES") && e.message.contains("item 2"),
            "{e}"
        );
    }

    #[test]
    fn a_mesh_that_changes_between_steps_is_an_error() {
        let a = fixture_grid();
        let mut b = a.clone();
        b.points[0][0] += 1.0;
        let (a, b) = (write_vtu(&a).unwrap(), write_vtu(&b).unwrap());
        let pvd = std::fs::read_to_string(fixture_dir().join("results.pvd")).unwrap();
        let e = load("results.pvd", &pvd, |f| {
            Ok(if f == "step-1.vtu" {
                a.clone()
            } else {
                b.clone()
            })
        })
        .unwrap_err();
        assert_eq!(
            (e.code, e.file.as_str()),
            ("results.mesh.changed", "step-2.vtu")
        );
    }

    #[test]
    fn a_grid_with_no_drawable_cell_is_an_error() {
        let mut g = fixture_grid();
        g.cell_types = vec![42; g.cells.len()];
        let e = one_step(&g).unwrap_err();
        assert_eq!(e.code, "results.mesh.unsupported");
    }
}
