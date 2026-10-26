//! Run results to view data: the steps of a `results.pvd`, the displacement of each step,
//! the contour fields, the deformed shape and the contour bands.
//!
//! Plain functions with no GPU or file types. The caller gives the file bytes to `load`. The
//! browser fetches them from the daemon, and the tests read them from disk.

use fh_core::vtu::{read_pvd, read_vtu, Grid};

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

/// An error text for a result file. It starts with the name of the file that has the problem.
fn fail<T>(file: &str, message: impl Into<String>) -> Result<T, String> {
    Err(format!("{file}: {}", message.into()))
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
    /// True for 1 component of a field with 2 or more components, for example `U1` or `S11`.
    pub component: bool,
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
    /// The time of this output of the solver, from `results.pvd`.
    pub time: f64,
    /// The VTU file name, from `results.pvd`.
    pub file: String,
    /// The index of the mesh of the step in [`Results::meshes`].
    pub mesh: usize,
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

/// All the steps of one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    /// The unit system of the run (`SI-mm`, `SI-m` or `unspecified`).
    pub units: String,
    /// True for the motion of rigid bodies (`kind="multibody"` in `results.pvd`).
    pub multibody: bool,
    /// The mesh of each different list of cells, in step order. The steps with the same cells
    /// share a mesh. A step of a staged model (`*MODEL CHANGE` of `ccx`) has only the elements
    /// in the model in that step.
    pub meshes: Vec<RenderMesh>,
    pub steps: Vec<Step>,
    /// True when the points of a step are not the points of the first step, for example the
    /// particles of a LAMMPS run. The displacement is then the true motion.
    pub moves: bool,
}

impl Results {
    /// The mesh of `step`.
    pub fn mesh(&self, step: &Step) -> &RenderMesh {
        &self.meshes[step.mesh]
    }

    /// The bounds of all the meshes.
    pub fn bounds(&self) -> Option<mesh::Bounds> {
        let corners: Vec<[f32; 3]> = (self.meshes.iter())
            .filter_map(|m| m.bounds)
            .flat_map(|b| [b.min, b.max])
            .collect();
        mesh::bounds(&corners)
    }
}

/// The attribute `name` of the `Collection` element, or `None` when it is absent.
fn pvd_attribute<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split_once("<Collection")
        .and_then(|(_, rest)| rest.split_once('>'))
        .and_then(|(attrs, _)| attrs.split_once(&format!(" {name}=\"")))
        .and_then(|(_, v)| v.split_once('"'))
        .map(|(v, _)| v)
}

/// The `units` attribute of the `Collection` element, or `unspecified` when it is absent.
fn pvd_units(text: &str) -> String {
    pvd_attribute(text, "units")
        .unwrap_or("unspecified")
        .to_owned()
}

/// The names of the components of a field with `components` values per item.
fn component_names(name: &str, components: usize) -> Vec<String> {
    match (name, components) {
        ("S" | "E", 6) => ["11", "22", "33", "12", "23", "13"]
            .map(|c| format!("{name}{c}"))
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
/// the components of `S`, and each scalar field.
fn fields_of(grid: &Grid, file: &str) -> Result<Vec<ScalarField>, String> {
    let mut out = Vec::new();
    let groups = [
        (Location::Point, &grid.point_data, grid.points.len()),
        (Location::Cell, &grid.cell_data, grid.cells.len()),
    ];
    for (location, data, count) in groups {
        for field in data.iter() {
            let values = &field.values;
            let c = field.components.max(1);
            if values.len() != c * count {
                return fail(
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
                    component: false,
                    values: values
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|v| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt())
                        .collect(),
                });
            }
            for (k, name) in component_names(&field.name, c).into_iter().enumerate() {
                out.push(ScalarField {
                    name,
                    quantity: q,
                    location,
                    component: c > 1,
                    values: values.iter().skip(k).step_by(c).copied().collect(),
                });
            }
        }
    }
    Ok(out)
}

/// True when `name` is a plain file name in the run folder: not empty, not `.` or `..`, and
/// with no `/`, `\` or `:`.
pub fn is_plain_file_name(name: &str) -> bool {
    !(name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']))
}

/// Read the results of a run. `pvd` is the text of `results.pvd`. `read` gives the bytes of a
/// file that `results.pvd` names. Each step must have as many points as the first step. A step
/// with other cells than the step before it gets its own mesh. A step with no displacement field
/// `U` (a heat transfer step) has a displacement of zero. When the points of a step are not the
/// points of the first step, the displacement of each node adds its move.
pub fn load(
    pvd_name: &str,
    pvd: &str,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Results, String> {
    let entries = match read_pvd(pvd) {
        Ok(e) => e,
        Err(e) => return fail(pvd_name, e),
    };
    if entries.is_empty() {
        return fail(pvd_name, "the collection names no step file");
    }
    let mut first: Option<Vec<[f64; 3]>> = None;
    // The cells and the cell types of the last mesh.
    let mut last: Option<(Vec<Vec<u32>>, Vec<u8>)> = None;
    let mut meshes = Vec::new();
    let mut steps = Vec::with_capacity(entries.len());
    let mut moves = false;
    for entry in entries {
        let file = entry.file.as_str();
        if !is_plain_file_name(file) {
            return fail(
                pvd_name,
                format!("step file {file} is not in the run folder"),
            );
        }
        let bytes = match read(file) {
            Ok(b) => b,
            Err(e) => return fail(file, e),
        };
        let grid = match read_vtu(&bytes) {
            Ok(g) => g,
            Err(e) => return fail(file, e),
        };
        if grid.points.is_empty() || grid.cells.is_empty() {
            return fail(file, "the grid has no point or no cell");
        }
        if first.as_ref().is_some_and(|f| f.len() != grid.points.len()) {
            return fail(
                file,
                format!("the points are not the points of {}", steps_file(&steps)),
            );
        }
        if grid.points.iter().flatten().any(|c| !c.is_finite()) {
            return fail(file, "a point coordinate is not a finite number");
        }
        let fields = fields_of(&grid, file)?;
        let mut displacement = match grid.point_field("U") {
            None => vec![[0.0; 3]; grid.points.len()],
            Some(u) if u.components == 3 => u.values.as_chunks::<3>().0.to_vec(),
            Some(_) => return fail(file, "the displacement field U must have 3 components"),
        };
        if let Some(f) = first.as_ref().filter(|f| **f != grid.points) {
            moves = true;
            for ((u, p), p0) in displacement.iter_mut().zip(&grid.points).zip(f) {
                for k in 0..3 {
                    u[k] += p[k] - p0[k];
                }
            }
        }
        let same = (last.as_ref()).is_some_and(|(c, t)| *c == grid.cells && *t == grid.cell_types);
        if !same {
            let mesh = mesh::build_grid(&grid);
            if mesh.triangles.is_empty() {
                return fail(
                    file,
                    format!(
                        "no cell type that the page draws ({} cells)",
                        grid.cells.len()
                    ),
                );
            }
            meshes.push(mesh);
            last = Some((grid.cells, grid.cell_types));
        }
        steps.push(Step {
            time: entry.time,
            file: entry.file.clone(),
            mesh: meshes.len() - 1,
            displacement,
            fields,
        });
        if first.is_none() {
            first = Some(grid.points);
        }
    }
    Ok(Results {
        units: pvd_units(pvd),
        multibody: pvd_attribute(pvd, "kind") == Some("multibody"),
        meshes,
        steps,
        moves,
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
/// `AUTO_SCALE_FRACTION` of the model size. For a multibody run it is never below 1, so the
/// motion of a mechanism shows at true scale. A model with no displacement gives 1, and a model
/// whose points move gives 1, the true motion.
pub fn auto_scale(results: &Results) -> f64 {
    if results.moves {
        return 1.0;
    }
    let size = results
        .bounds()
        .map_or(0.0, |b| 2.0 * f64::from(b.radius()));
    let max = results
        .steps
        .iter()
        .map(Step::max_displacement)
        .fold(0.0, f64::max);
    if max > 0.0 {
        let scale = nice_scale(AUTO_SCALE_FRACTION * size / max);
        if results.multibody {
            scale.max(1.0)
        } else {
            scale
        }
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

/// The bounds of the first view: the bounds of all the meshes. When the points move, the bounds
/// also hold the moved points of all steps, at scale 1.
pub fn view_bounds(results: &Results) -> Option<mesh::Bounds> {
    if !results.moves {
        return results.bounds();
    }
    let mut points: Vec<[f32; 3]> = (results.meshes.iter())
        .flat_map(|m| m.positions.iter().copied())
        .collect();
    for step in &results.steps {
        points.extend(deformed_positions(results.mesh(step), step, 1.0));
    }
    mesh::bounds(&points)
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
    collapse(lo, hi)
}

/// `(lo, hi)`, or one value (the middle) when the spread is round-off.
fn collapse(lo: f64, hi: f64) -> (f64, f64) {
    if hi - lo <= CONSTANT_SPREAD * lo.abs().max(hi.abs()) {
        let mid = 0.5 * (lo + hi);
        (mid, mid)
    } else {
        (lo, hi)
    }
}

/// The position of a value in the range `lo..=hi`: 0 at `lo`, 1 at `hi`, 0.5 for a range
/// of zero width. Linear in the value, so it can be interpolated over a triangle.
pub fn position(value: f64, lo: f64, hi: f64) -> f64 {
    if hi <= lo {
        return 0.5;
    }
    (value - lo) / (hi - lo)
}

/// The contour position (see `position`) at each corner of each surface triangle, three per
/// triangle. A point field gives the node value. A cell field gives the value of the owning
/// element.
pub fn triangle_values(mesh: &RenderMesh, field: &ScalarField, lo: f64, hi: f64) -> Vec<f32> {
    let mut out = Vec::with_capacity(3 * mesh.triangles.len());
    for (t, cell) in mesh.triangles.iter().zip(&mesh.triangle_cell) {
        for &p in t {
            let v = match field.location {
                Location::Point => field.values[mesh.source_node[p as usize] as usize],
                Location::Cell => field.values[*cell as usize],
            };
            out.push(position(v, lo, hi) as f32);
        }
    }
    out
}

/// The band (0 to `BANDS - 1`) of a value in the range `lo..=hi`. The shader maps the
/// position of `triangle_values` to a band with the same rule. A range of zero width gives
/// the middle band.
pub fn band(value: f64, lo: f64, hi: f64) -> usize {
    let t = position(value, lo, hi);
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
    use std::path::PathBuf;

    use fh_core::vtu::Field;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/results/cantilever-2step")
    }

    #[test]
    fn fixture_has_two_steps_with_units_and_times() {
        let r = crate::load_run(&fixture_dir()).unwrap();
        assert_eq!(r.units, "SI-mm");
        assert_eq!(r.steps.len(), 2);
        assert_eq!((r.steps[0].time, r.steps[1].time), (1.0, 2.0));
        assert_eq!(r.steps[1].file, "step-2.vtu");
        assert_eq!(
            (r.meshes[0].node_count, r.meshes[0].element_count),
            (81, 24)
        );
        assert_eq!(r.meshes[0].skipped_elements, 0);
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

    /// Each displacement and each contour value matches the arrays of the VTU file of the
    /// same step, item by item.
    #[test]
    fn values_match_the_vtu_arrays() {
        let dir = fixture_dir();
        let r = crate::load_run(&dir).unwrap();
        for step in &r.steps {
            let grid = read_vtu(&std::fs::read(dir.join(&step.file)).unwrap()).unwrap();
            let array = |f: Option<&Field>| f.unwrap().values.clone();
            let (u, rf) = (array(grid.point_field("U")), array(grid.point_field("RF")));
            let (s, mises) = (
                array(grid.point_field("S")),
                array(grid.point_field("MISES")),
            );
            let comp = |name: &str| &step.field(name).unwrap().values;
            let mag = |v: &[f64]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let close = |got: f64, want: f64, what: &str| {
                assert!((got - want).abs() <= 1e-15 * want.abs().max(1.0), "{what}");
            };
            assert_eq!(step.displacement.len(), grid.points.len());
            for n in 0..grid.points.len() {
                let (u, rf) = (&u[3 * n..3 * n + 3], &rf[3 * n..3 * n + 3]);
                assert_eq!(step.displacement[n], [u[0], u[1], u[2]], "U point {n}");
                for (k, name) in ["U1", "U2", "U3", "RF1", "RF2", "RF3"].iter().enumerate() {
                    let want = if k < 3 { u[k] } else { rf[k - 3] };
                    assert_eq!(comp(name)[n], want, "{name} point {n}");
                }
                close(comp("U magnitude")[n], mag(u), "U magnitude");
                close(comp("RF magnitude")[n], mag(rf), "RF magnitude");
            }
            // The VTU order of S is xx, yy, zz, xy, yz, xz.
            let names = ["S11", "S22", "S33", "S12", "S23", "S13"];
            for e in 0..mises.len() {
                for (k, name) in names.iter().enumerate() {
                    assert_eq!(comp(name)[e], s[6 * e + k], "{name} point {e}");
                }
                assert_eq!(comp("MISES")[e], mises[e], "MISES point {e}");
            }
        }
        // The second step doubles the load of the linear model.
        let (a, b) = (&r.steps[0], &r.steps[1]);
        let ratio = b.max_displacement() / a.max_displacement();
        assert!((ratio - 2.0).abs() < 1e-9, "ratio {ratio}");
    }

    /// For a multibody run, a displacement larger than a tenth of the model (the motion of a
    /// mechanism) shows at true scale. A small displacement shows larger.
    #[test]
    fn the_automatic_scale_of_a_multibody_run_is_never_below_1() {
        let mut r = crate::load_run(&fixture_dir()).unwrap();
        r.multibody = true;
        assert!(auto_scale(&r) > 1.0);
        for step in &mut r.steps {
            step.displacement
                .iter_mut()
                .flatten()
                .for_each(|u| *u *= 1e3);
        }
        assert_eq!(auto_scale(&r), 1.0);
    }

    /// For a run that is not multibody (for example a mode shape of ccx), a displacement larger
    /// than a tenth of the model shows smaller than true scale.
    #[test]
    fn the_automatic_scale_of_a_large_ccx_displacement_is_below_1() {
        let mut r = crate::load_run(&fixture_dir()).unwrap();
        assert!(!r.multibody);
        for step in &mut r.steps {
            step.displacement
                .iter_mut()
                .flatten()
                .for_each(|u| *u *= 1e3);
        }
        assert!(auto_scale(&r) < 1.0, "{}", auto_scale(&r));
    }

    #[test]
    fn deformed_positions_add_the_scaled_displacement() {
        let r = crate::load_run(&fixture_dir()).unwrap();
        let step = &r.steps[1];
        let scale = 50.0;
        let moved = deformed_positions(&r.meshes[0], step, scale);
        assert_eq!(moved.len(), r.meshes[0].positions.len());
        for (i, p) in moved.iter().enumerate() {
            let u = step.displacement[r.meshes[0].source_node[i] as usize];
            for k in 0..3 {
                let want = f64::from(r.meshes[0].positions[i][k]) + scale * u[k];
                assert!((f64::from(p[k]) - want).abs() < 1e-5, "position {i}");
            }
        }
        // The tip (x = 10) moves down in z by 50 x the tip U3 of step 2.
        let tip = r.meshes[0]
            .positions
            .iter()
            .position(|p| p[0] == 10.0)
            .unwrap();
        let u3 = step.displacement[r.meshes[0].source_node[tip] as usize][2];
        assert!((u3 - -0.0814817).abs() < 1e-6, "{u3}");
        assert!(
            (f64::from(moved[tip][2] - r.meshes[0].positions[tip][2]) - scale * u3).abs() < 1e-5
        );
        // Scale 0 is the undeformed shape.
        assert_eq!(
            deformed_positions(&r.meshes[0], step, 0.0),
            r.meshes[0].positions
        );
    }

    /// LAMMPS particles move: the displacement is the move of the points, at the true scale.
    #[test]
    fn moved_points_are_the_displacement_at_scale_1() {
        let dir = fixture_dir().join("../particles-2step");
        let r = crate::load_run(&dir).unwrap();
        assert!(r.moves && !crate::load_run(&fixture_dir()).unwrap().moves);
        assert_eq!(auto_scale(&r), 1.0);
        assert!(r.steps[0].displacement.iter().all(|u| *u == [0.0; 3]));
        // Both particles move 0.5 in x from time 0 to time 0.5.
        assert!(r.steps[1]
            .displacement
            .iter()
            .all(|u| *u == [0.5, 0.0, 0.0]));
        let grid = read_vtu(&std::fs::read(dir.join("step-2.vtu")).unwrap()).unwrap();
        let moved = deformed_positions(&r.meshes[0], &r.steps[1], 1.0);
        for (i, p) in moved.iter().enumerate() {
            let want = grid.points[r.meshes[0].source_node[i] as usize];
            assert_eq!(p.map(f64::from), want, "position {i}");
        }
    }

    /// The first view holds the particles of all steps, and not only of the first step.
    #[test]
    fn view_bounds_hold_the_moved_points_of_all_steps() {
        let r = crate::load_run(&fixture_dir().join("../particles-2step")).unwrap();
        let first = r.meshes[0].bounds.unwrap();
        let b = view_bounds(&r).unwrap();
        assert_eq!(b.min, first.min);
        assert_eq!(b.max, [first.max[0] + 0.5, first.max[1], first.max[2]]);
        let fixed = crate::load_run(&fixture_dir()).unwrap();
        assert_eq!(view_bounds(&fixed), fixed.meshes[0].bounds);
    }

    /// The positions that the shader bands agree with the bands of the legend.
    #[test]
    fn positions_band_as_the_legend_does() {
        let mesh = RenderMesh {
            positions: vec![[0.0; 3]; 3],
            triangles: vec![[0, 1, 2]],
            triangle_cell: vec![0],
            source_node: vec![0, 1, 2],
            ..Default::default()
        };
        let band_of = |t: f32| ((t as f64 * BANDS as f64).floor().max(0.0) as usize).min(BANDS - 1);
        let mises = ScalarField {
            name: "MISES".into(),
            quantity: Quantity::Stress,
            location: Location::Point,
            component: false,
            values: vec![100.0 - 4e-6, 100.0, 100.0 + 4e-6],
        };
        let (lo, hi) = contour_range(&mises);
        let t = triangle_values(&mesh, &mises, lo, hi);
        assert_eq!(
            t.iter().map(|&t| band_of(t)).collect::<Vec<_>>(),
            mises
                .values
                .iter()
                .map(|&v| band(v, lo, hi))
                .collect::<Vec<_>>()
        );
        assert_eq!(band_of(t[0]), 0);
        assert_eq!(band_of(t[2]), BANDS - 1);
        let tiny = ScalarField {
            values: vec![0.0, 5e-301, 1e-300],
            ..mises
        };
        let (lo, hi) = contour_range(&tiny);
        let t = triangle_values(&mesh, &tiny, lo, hi);
        assert_eq!(t.iter().map(|&t| band_of(t)).collect::<Vec<_>>(), [0, 4, 8]);
        let constant = triangle_values(&mesh, &tiny, 3.0, 3.0);
        assert!(constant.iter().all(|&t| band_of(t) == BANDS / 2));
    }

    #[test]
    fn units_follow_the_unit_system() {
        assert_eq!(unit("SI-mm", Quantity::Stress), "MPa");
        assert_eq!(unit("SI-m", Quantity::Stress), "Pa");
        assert_eq!(unit("SI-mm", Quantity::Displacement), "mm");
        assert_eq!(unit("SI-m", Quantity::Force), "N");
        assert_eq!(unit("unspecified", Quantity::Stress), "");
        assert_eq!(pvd_units("<Collection>"), "unspecified");
        assert_eq!(
            pvd_units("<Collection kind=\"multibody\" units=\"SI-m\">"),
            "SI-m"
        );
    }
}

/// The finite range of each field over all steps. Absent fields contribute no values.
pub fn all_ranges(results: &Results) -> Vec<(String, Location, f64, f64)> {
    let mut ranges: Vec<(String, Location, f64, f64)> = Vec::new();
    for field in results.steps.iter().flat_map(|s| &s.fields) {
        let Some((lo, hi)) = field
            .values
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .map(|v| (v, v))
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
        else {
            continue;
        };
        if let Some((_, _, a, b)) = ranges
            .iter_mut()
            .find(|(n, l, _, _)| n == &field.name && *l == field.location)
        {
            *a = a.min(lo);
            *b = b.max(hi);
        } else {
            ranges.push((field.name.clone(), field.location, lo, hi));
        }
    }
    for (_, _, lo, hi) in &mut ranges {
        (*lo, *hi) = collapse(*lo, *hi);
    }
    ranges
}
