//! Run results to view data: the steps of a `results.pvd`, the displacement of each step,
//! the contour fields, the deformed shape and the contour bands.
//!
//! Plain functions with no GPU or file types. The caller gives the file bytes to `load`. The
//! browser fetches them from the daemon, and the tests read them from disk.

use bytemuck::Pod;
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
#[derive(Debug, Clone, Default, PartialEq)]
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
/// the components of `S`, and each scalar field. Takes the fields out of the grid, and frees
/// each one after its contour fields are made. Reason: a copy of all the fields of an A320
/// fuselage is 34 MB more in a phone browser.
fn fields_of(grid: &mut Grid, file: &str) -> Result<Vec<ScalarField>, String> {
    let mut out = Vec::new();
    let groups = [
        (
            Location::Point,
            std::mem::take(&mut grid.point_data),
            grid.points.len(),
        ),
        (
            Location::Cell,
            std::mem::take(&mut grid.cell_data),
            grid.cells.len(),
        ),
    ];
    for (location, data, count) in groups {
        for field in data {
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
            let q = if location == Location::Cell && field.name == "U" {
                Quantity::Other
            } else {
                quantity(&field.name)
            };
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
/// file that `results.pvd` names. See [`Loader`] for the checks of each step.
pub fn load(
    pvd_name: &str,
    pvd: &str,
    mut read: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Results, String> {
    let mut loader = Loader::new(pvd_name, pvd)?;
    while let Some(file) = loader.next_file() {
        let file = file.to_owned();
        loader.add(read(&file))?;
    }
    Ok(loader.finish())
}

/// Reads the results of a run 1 step at a time, so that the page can fetch a step, read it
/// and give the browser a frame before the next step. Each step must have as many points as
/// the first step. A step with other cells than the step before it gets its own mesh. A step
/// with no displacement field `U` (a heat transfer step) has a displacement of zero. When the
/// points of a step are not the points of the first step, the displacement of each node adds
/// its move.
pub struct Loader {
    units: String,
    multibody: bool,
    entries: Vec<fh_core::vtu::PvdEntry>,
    first: Option<Vec<[f64; 3]>>,
    /// The cells and the cell types of the last mesh.
    last: Option<(Vec<Vec<u32>>, Vec<u8>)>,
    meshes: Vec<RenderMesh>,
    steps: Vec<Step>,
    moves: bool,
    /// The number of meshes that [`Loader::add_and_send`] sent.
    sent: usize,
}

impl Loader {
    /// Read `pvd`, the text of the collection `pvd_name`.
    pub fn new(pvd_name: &str, pvd: &str) -> Result<Loader, String> {
        let entries = match read_pvd(pvd) {
            Ok(e) => e,
            Err(e) => return fail(pvd_name, e),
        };
        if entries.is_empty() {
            return fail(pvd_name, "the collection names no step file");
        }
        if let Some(e) = entries.iter().find(|e| !is_plain_file_name(&e.file)) {
            return fail(
                pvd_name,
                format!("step file {} is not in the run folder", e.file),
            );
        }
        let steps = Vec::with_capacity(entries.len());
        Ok(Loader {
            units: pvd_units(pvd),
            multibody: pvd_attribute(pvd, "kind") == Some("multibody"),
            entries,
            first: None,
            last: None,
            meshes: Vec::new(),
            steps,
            moves: false,
            sent: 0,
        })
    }

    /// A loader of steps that come 1 at a time with no `results.pvd`, for example from the
    /// browser solve. [`Loader::push_and_send`] gives each step.
    pub fn stream(units: &str) -> Loader {
        Loader {
            units: units.to_owned(),
            multibody: false,
            entries: Vec::new(),
            first: None,
            last: None,
            meshes: Vec::new(),
            steps: Vec::new(),
            moves: false,
            sent: 0,
        }
    }

    /// The file of the next step, or `None` when each step is read.
    pub fn next_file(&self) -> Option<&str> {
        self.entries.get(self.steps.len()).map(|e| e.file.as_str())
    }

    /// Read the bytes of the file of the next step.
    pub fn add(&mut self, bytes: Result<Vec<u8>, String>) -> Result<(), String> {
        let Some(entry) = self.entries.get(self.steps.len()) else {
            return Err("each step is already read".into());
        };
        let file = entry.file.as_str();
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => return fail(file, e),
        };
        let grid = match read_vtu(&bytes) {
            Ok(g) => g,
            Err(e) => return fail(file, e),
        };
        drop(bytes);
        self.add_grid(grid)
    }

    /// Read the grid of the next step.
    fn add_grid(&mut self, mut grid: Grid) -> Result<(), String> {
        let entry = &self.entries[self.steps.len()];
        let file = entry.file.as_str();
        if grid.points.is_empty() || grid.cells.is_empty() {
            return fail(file, "the grid has no point or no cell");
        }
        if (self.first.as_ref()).is_some_and(|f| f.len() != grid.points.len()) {
            return fail(
                file,
                format!(
                    "the points are not the points of {}",
                    steps_file(&self.steps)
                ),
            );
        }
        if grid.points.iter().flatten().any(|c| !c.is_finite()) {
            return fail(file, "a point coordinate is not a finite number");
        }
        // The mesh, the displacement and then the fields, so that a phone browser does not
        // hold the work data of the mesh and a second copy of the fields at the same time.
        let same =
            (self.last.as_ref()).is_some_and(|(c, t)| *c == grid.cells && *t == grid.cell_types);
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
            self.meshes.push(mesh);
        }
        let mut displacement = match grid.point_field("U") {
            None => vec![[0.0; 3]; grid.points.len()],
            Some(u) if u.components == 3 => u.values.as_chunks::<3>().0.to_vec(),
            Some(_) => return fail(file, "the displacement field U must have 3 components"),
        };
        if let Some(f) = self.first.as_ref().filter(|f| **f != grid.points) {
            self.moves = true;
            for ((u, p), p0) in displacement.iter_mut().zip(&grid.points).zip(f) {
                for k in 0..3 {
                    u[k] += p[k] - p0[k];
                }
            }
        }
        let fields = fields_of(&mut grid, file)?;
        if !same {
            self.last = Some((grid.cells, grid.cell_types));
        }
        self.steps.push(Step {
            time: entry.time,
            file: entry.file.clone(),
            mesh: self.meshes.len() - 1,
            displacement,
            fields,
        });
        if self.first.is_none() {
            self.first = Some(grid.points);
        }
        Ok(())
    }

    /// Read the next step like [`Loader::add`] and give it, with its mesh when the mesh is new,
    /// as the lists of [`Results::receive`]. The worker of the page reads the steps with this,
    /// so that a large step does not stop the page. The loader then keeps only what it needs to
    /// check the next steps, so [`Loader::finish`] has no step data after this.
    pub fn add_and_send(&mut self, bytes: Result<Vec<u8>, String>) -> Result<Vec<Vec<u8>>, String> {
        self.add(bytes)?;
        self.send()
    }

    /// Add the step `entry` with its grid, and give it like [`Loader::add_and_send`].
    pub fn push_and_send(
        &mut self,
        entry: fh_core::vtu::PvdEntry,
        grid: Grid,
    ) -> Result<Vec<Vec<u8>>, String> {
        self.entries.push(entry);
        self.add_grid(grid)?;
        self.send()
    }

    /// The lists of the last step that the loader read.
    fn send(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let mut out = vec![Vec::new()];
        put_text(&mut out, &self.units);
        put(&mut out, u8::from(self.multibody));
        put(&mut out, u8::from(self.moves));
        let mesh = self.meshes.get_mut(self.sent).map(std::mem::take);
        self.sent = self.meshes.len();
        put(&mut out, u8::from(mesh.is_some()));
        if let Some(m) = mesh {
            put_list(&mut out, &m.positions);
            put_list(&mut out, &m.source_node);
            put_list(&mut out, &m.triangles);
            put_list(&mut out, &m.triangle_cell);
            put_list(&mut out, &m.edges);
            put_list(&mut out, &m.feature_edges);
            for n in [m.node_count, m.element_count, m.skipped_elements] {
                put(&mut out, n as u64);
            }
            put(&mut out, u8::from(m.bounds.is_some()));
            if let Some(b) = m.bounds {
                put(&mut out, b.min);
                put(&mut out, b.max);
            }
        }
        let step = self.steps.last_mut().ok_or("no step is read")?;
        put(&mut out, step.time);
        put_text(&mut out, &step.file);
        put(&mut out, step.mesh as u64);
        put_list(&mut out, &std::mem::take(&mut step.displacement));
        let fields = std::mem::take(&mut step.fields);
        put(&mut out, fields.len() as u64);
        for f in fields {
            put_text(&mut out, &f.name);
            put(&mut out, f.quantity as u8);
            put(&mut out, f.location as u8);
            put(&mut out, u8::from(f.component));
            put_list(&mut out, &f.values);
        }
        Ok(out)
    }

    /// The results, after each step is read.
    pub fn finish(self) -> Results {
        Results {
            units: self.units,
            multibody: self.multibody,
            meshes: self.meshes,
            steps: self.steps,
            moves: self.moves,
        }
    }
}

impl Results {
    /// Add the step that [`Loader::add_and_send`] gave as `lists`. Each list of numbers is a
    /// copy of its bytes, with no work for each element, so that a large step does not stop
    /// the page.
    pub fn receive<L: Lists + ?Sized>(&mut self, lists: &L) -> Result<(), String> {
        let head = list::<u8, _>(lists, 0)?;
        let mut r = Received {
            head: &head,
            lists,
            next: 1,
        };
        self.units = r.text()?;
        self.multibody = r.get::<u8>()? != 0;
        self.moves = r.get::<u8>()? != 0;
        if r.get::<u8>()? != 0 {
            self.meshes.push(RenderMesh {
                positions: r.list()?,
                source_node: r.list()?,
                triangles: r.list()?,
                triangle_cell: r.list()?,
                edges: r.list()?,
                feature_edges: r.list()?,
                node_count: r.get::<u64>()? as usize,
                element_count: r.get::<u64>()? as usize,
                skipped_elements: r.get::<u64>()? as usize,
                bounds: match r.get::<u8>()? {
                    0 => None,
                    _ => Some(mesh::Bounds {
                        min: r.get()?,
                        max: r.get()?,
                    }),
                },
            });
        }
        let time = r.get()?;
        let file = r.text()?;
        let mesh = r.get::<u64>()? as usize;
        let displacement = r.list()?;
        let mut fields = Vec::new();
        for _ in 0..r.get::<u64>()? {
            fields.push(ScalarField {
                name: r.text()?,
                quantity: *[
                    Quantity::Displacement,
                    Quantity::Force,
                    Quantity::Stress,
                    Quantity::Other,
                ]
                .get(usize::from(r.get::<u8>()?))
                .ok_or(BAD_STEP)?,
                location: *[Location::Point, Location::Cell]
                    .get(usize::from(r.get::<u8>()?))
                    .ok_or(BAD_STEP)?,
                component: r.get::<u8>()? != 0,
                values: r.list()?,
            });
        }
        if mesh >= self.meshes.len() || !r.head.is_empty() {
            return Err(BAD_STEP.into());
        }
        self.steps.push(Step {
            time,
            file,
            mesh,
            displacement,
            fields,
        });
        Ok(())
    }
}

/// The error of [`Results::receive`] for lists that [`Loader::add_and_send`] did not write.
const BAD_STEP: &str = "the step data from the worker is not complete";

/// The byte lists of 1 step from [`Loader::add_and_send`]: the small values first, then each
/// list of numbers. The page gives the buffers that the worker transferred.
pub trait Lists {
    /// The number of bytes of list `i`, or `None` when there is no list `i`.
    fn size(&self, i: usize) -> Option<usize>;
    /// Copy the bytes of list `i` into `out`, which has [`Lists::size`] bytes.
    fn copy(&self, i: usize, out: &mut [u8]);
}

impl Lists for [Vec<u8>] {
    fn size(&self, i: usize) -> Option<usize> {
        self.get(i).map(Vec::len)
    }

    fn copy(&self, i: usize, out: &mut [u8]) {
        out.copy_from_slice(&self[i]);
    }
}

/// List `i` of `lists` as values of `T`.
fn list<T: Pod, L: Lists + ?Sized>(lists: &L, i: usize) -> Result<Vec<T>, String> {
    let size = lists.size(i).ok_or(BAD_STEP)?;
    if size % size_of::<T>() != 0 {
        return Err(BAD_STEP.into());
    }
    let mut values = vec![T::zeroed(); size / size_of::<T>()];
    lists.copy(i, bytemuck::cast_slice_mut(&mut values));
    Ok(values)
}

fn put<T: Pod>(out: &mut [Vec<u8>], value: T) {
    out[0].extend_from_slice(bytemuck::bytes_of(&value));
}

fn put_list<T: Pod>(out: &mut Vec<Vec<u8>>, values: &[T]) {
    out.push(bytemuck::cast_slice(values).to_vec());
}

fn put_text(out: &mut [Vec<u8>], text: &str) {
    put(out, text.len() as u64);
    out[0].extend_from_slice(text.as_bytes());
}

/// The lists of [`Loader::add_and_send`] that [`Results::receive`] did not read yet.
struct Received<'a, L: ?Sized> {
    head: &'a [u8],
    lists: &'a L,
    next: usize,
}

impl<L: Lists + ?Sized> Received<'_, L> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if n > self.head.len() {
            return Err(BAD_STEP.into());
        }
        let (taken, rest) = self.head.split_at(n);
        self.head = rest;
        Ok(taken)
    }

    fn get<T: Pod>(&mut self) -> Result<T, String> {
        Ok(bytemuck::pod_read_unaligned(self.take(size_of::<T>())?))
    }

    fn list<T: Pod>(&mut self) -> Result<Vec<T>, String> {
        self.next += 1;
        list(self.lists, self.next - 1)
    }

    fn text(&mut self) -> Result<String, String> {
        let n = usize::try_from(self.get::<u64>()?).map_err(|_| BAD_STEP)?;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| BAD_STEP.into())
    }
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

    #[test]
    fn cell_velocity_is_not_structural_displacement() {
        let mut grid = Grid::default();
        grid.cells.push(vec![0]);
        grid.cell_data.push(Field {
            name: "U".into(),
            components: 3,
            values: vec![1.0, 2.0, 0.0],
        });
        let fields = fields_of(&mut grid, "fluid.vtu").unwrap();
        assert_ne!(fields[0].quantity, Quantity::Displacement);
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

    /// The steps that the worker of the page sends give the results that `load` gives, also
    /// for points that move. A fetch error keeps the text of `load`, and a cut message fails.
    #[test]
    fn sent_steps_are_the_loaded_results() {
        let mut first = Vec::new();
        for dir in [fixture_dir(), fixture_dir().join("../particles-2step")] {
            let pvd = std::fs::read_to_string(dir.join(crate::RESULTS_FILE)).unwrap();
            let mut loader = Loader::new(crate::RESULTS_FILE, &pvd).unwrap();
            let mut received = Results::default();
            while let Some(file) = loader.next_file() {
                let bytes = std::fs::read(dir.join(file)).map_err(|e| e.to_string());
                let sent = loader.add_and_send(bytes).unwrap();
                received.receive(&sent[..]).unwrap();
                if first.is_empty() {
                    first = sent;
                }
            }
            assert_eq!(
                received,
                crate::load_run(&dir).unwrap(),
                "{}",
                dir.display()
            );
        }
        for cut in [0, 1, first[0].len() / 2, first[0].len() - 1] {
            let mut cut_head = first.clone();
            cut_head[0].truncate(cut);
            assert_eq!(
                Results::default().receive(&cut_head[..]),
                Err(BAD_STEP.into())
            );
        }
        assert_eq!(
            Results::default().receive(&first[..first.len() - 1]),
            Err(BAD_STEP.into())
        );
        let pvd = r#"<VTKFile type="Collection"><Collection><DataSet timestep="1" file="a.vtu"/></Collection></VTKFile>"#;
        let mut loader = Loader::new(crate::RESULTS_FILE, pvd).unwrap();
        assert_eq!(
            loader.add_and_send(Err("HTTP 404: no such file".into())),
            Err("a.vtu: HTTP 404: no such file".into())
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
