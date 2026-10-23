//! The result view state: the selected step, the deformed shape and the contour field. It
//! makes the vertex data for the GPU layer. Plain functions with no GPU types.
//!
//! A report gives the view of a run ([`View`]) that the agent writes for the story of the
//! report: the fields, the controls, the step, the scale and the box that the camera shows.
//! A run with no view shows each control that can change its view.

use crate::mesh::{Bounds, RenderMesh};
use crate::results::{self, Results, ScalarField, Step};

/// The field that the contour shows first, when the run has it: the density of a topology run,
/// the von Mises stress, the water of a free surface flow, the Mach number of a compressible
/// flow, the displacement or the velocity magnitude, and the temperature.
const DEFAULT_FIELDS: [&str; 6] = ["density", "MISES", "alpha.water", "Ma", "U magnitude", "NT"];

/// A group of controls of the results panel. The contour field list always shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// The step selector, the play button and the step slider.
    Steps,
    /// The deformed shape, its scale and the undeformed outline.
    Deformation,
    /// The supports and loads of the input, on the model.
    Loads,
}

impl Control {
    pub const ALL: [Control; 3] = [Control::Steps, Control::Deformation, Control::Loads];

    /// The name in a view: `steps`, `deformation` or `loads`.
    pub fn name(self) -> &'static str {
        match self {
            Control::Steps => "steps",
            Control::Deformation => "deformation",
            Control::Loads => "loads",
        }
    }

    pub fn from_name(name: &str) -> Option<Control> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
}

/// The view of a run that the agent gives in a report. A `None` or an empty part comes from
/// the run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// The fields of the contour list. The first shows first.
    pub fields: Vec<String>,
    /// The groups of controls that show.
    pub controls: Option<Vec<Control>>,
    /// The index of the step that shows first.
    pub step: Option<usize>,
    /// The deformation scale at the start. 0 shows the undeformed shape.
    pub scale: Option<f64>,
    /// The box that the camera shows first.
    pub frame: Option<Bounds>,
}

impl View {
    /// Read a view: 1 part on each line, as the `data-` attributes of the report
    /// (`crates/fh/src/daemon/report.html`). `fields=NAME,NAME`, `controls=` with the names of
    /// the groups, `step=N` (from 1), `scale=X` and `box=X0,Y0,Z0,X1,Y1,Z1`.
    pub fn parse(text: &str) -> Result<View, String> {
        let mut view = View::default();
        fn list(value: &str) -> impl Iterator<Item = &str> {
            value.split(',').map(str::trim).filter(|n| !n.is_empty())
        }
        for line in text.lines().filter(|l| !l.is_empty()) {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("view line {line:?} has no ="))?;
            match key {
                "fields" => view.fields = list(value).map(str::to_owned).collect(),
                "controls" => {
                    let controls = list(value)
                        .map(|n| Control::from_name(n).ok_or_else(|| format!("no control {n:?}")))
                        .collect::<Result<_, _>>()?;
                    view.controls = Some(controls);
                }
                "step" => match value.parse::<usize>() {
                    Ok(n) if n > 0 => view.step = Some(n - 1),
                    _ => return Err(format!("step {value:?} is not a step number from 1")),
                },
                "scale" => match value.parse::<f64>() {
                    Ok(x) if x.is_finite() && x >= 0.0 => view.scale = Some(x),
                    _ => return Err(format!("scale {value:?} is not a number of 0 or more")),
                },
                "box" => {
                    let v: Vec<f32> = list(value).filter_map(|x| x.parse().ok()).collect();
                    match v[..] {
                        [x0, y0, z0, x1, y1, z1] if v.iter().all(|x| x.is_finite()) => {
                            view.frame = Some(Bounds {
                                min: [x0.min(x1), y0.min(y1), z0.min(z1)],
                                max: [x0.max(x1), y0.max(y1), z0.max(z1)],
                            });
                        }
                        _ => return Err(format!("box {value:?} is not 6 numbers")),
                    }
                }
                _ => return Err(format!("view line {line:?} has no known key")),
            }
        }
        Ok(view)
    }
}

/// What the results panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Panel {
    /// The groups of controls that show.
    pub controls: Vec<Control>,
    /// The fields of the contour list.
    pub fields: Vec<String>,
}

impl Panel {
    /// The panel of a run with the view `view`: each group of controls of the view (each group
    /// when the view names none) that can change the view of the run, and the fields of the
    /// view (each field of the run when the view names none). `loads` is true when the input of
    /// the run has a support or a load.
    pub fn of(results: &Results, view: &View, loads: bool) -> Panel {
        let can = [results.steps.len() > 1, displaced(results), loads];
        let controls = Control::ALL
            .into_iter()
            .zip(can)
            .filter(|(c, on)| *on && view.controls.as_ref().is_none_or(|v| v.contains(c)))
            .map(|(c, _)| c)
            .collect();
        let fields = if view.fields.is_empty() {
            field_names(results)
        } else {
            view.fields.clone()
        };
        Panel { controls, fields }
    }
}

/// True when the nodes have a displacement that the deformation scale changes. The points of
/// LAMMPS particles and of a moving OpenFOAM mesh move: the shape shows their true motion.
fn displaced(results: &Results) -> bool {
    !results.moves && results.steps.iter().any(|s| s.max_displacement() > 0.0)
}

/// The time in seconds that a playing animation shows each step.
pub const PLAY_SECONDS: f64 = 0.25;

/// New vertex data: the drawn position of each render position, and the position and the
/// contour position (0 to 1) of each triangle corner (three per triangle, see `face_vertices`).
pub struct Geometry {
    /// A new number for each change. The buffers are written once per number.
    pub version: u64,
    pub positions: Vec<[f32; 3]>,
    pub faces: Vec<[f32; 4]>,
}

/// The face vertex data: three corners per triangle, each `[x, y, z, value]`.
pub fn face_vertices(mesh: &RenderMesh, positions: &[[f32; 3]], values: &[f32]) -> Vec<[f32; 4]> {
    mesh.triangles
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, &p)| {
            let x = positions[p as usize];
            [x[0], x[1], x[2], values.get(i).copied().unwrap_or(0.0)]
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    /// The index of the selected step.
    pub step: usize,
    /// Draw the deformed shape.
    pub deformed: bool,
    /// The deformation scale factor.
    pub scale: f64,
    /// Draw the outline of the undeformed shape with the deformed shape.
    pub ghost: bool,
    /// The contour field, or `None` for plain faces.
    pub field: Option<String>,
    /// While the steps play: the time (in seconds) when the current step started to show.
    pub playing: Option<f64>,
}

/// What the contour shows for the selected step.
#[derive(Debug, Clone, PartialEq)]
pub enum Contour<'a> {
    Off,
    On {
        field: &'a ScalarField,
        lo: f64,
        hi: f64,
    },
    /// The selected step has no field with this name.
    Missing(&'a str),
}

impl ViewState {
    /// The first view state of a run: the step of the view, else the last step. The contour is
    /// the first field of the view, else the first field of `DEFAULT_FIELDS` in the step, else
    /// the first field of the panel in the step. The scale of the view, else the automatic
    /// scale when the panel shows the deformation, else 1 (the true shape).
    pub fn open(results: &Results, view: &View, panel: &Panel) -> Self {
        let last = results.steps.len().saturating_sub(1);
        let step = view.step.unwrap_or(last).min(last);
        let has = |name: &&str| results.steps[step].field(name).is_some();
        let field = view.fields.first().cloned().or_else(|| {
            let fields = || panel.fields.iter().map(String::as_str);
            DEFAULT_FIELDS
                .into_iter()
                .find(|d| fields().any(|f| f == *d) && has(d))
                .or_else(|| fields().find(has))
                .map(str::to_owned)
        });
        let auto = results::auto_scale(results);
        let (deformed, scale) = match view.scale {
            Some(0.0) => (false, auto),
            Some(s) => (true, s),
            None if panel.controls.contains(&Control::Deformation) => (true, auto),
            None => (true, 1.0),
        };
        ViewState {
            step,
            deformed,
            scale,
            ghost: true,
            field,
            playing: None,
        }
    }

    /// At the time `now` of a playing animation, go to the next step when the current step has
    /// shown for `PLAY_SECONDS`. The first step comes after the last. Gives the time to the
    /// next change, or `None` when the steps do not play.
    pub fn play(&mut self, now: f64, steps: usize) -> Option<f64> {
        let since = self.playing?;
        if now - since < PLAY_SECONDS {
            return Some(since + PLAY_SECONDS - now);
        }
        self.step = (self.step + 1) % steps.max(1);
        self.playing = Some(now);
        Some(PLAY_SECONDS)
    }

    pub fn current<'a>(&self, results: &'a Results) -> &'a Step {
        &results.steps[self.step.min(results.steps.len() - 1)]
    }

    pub fn contour<'a>(&'a self, results: &'a Results) -> Contour<'a> {
        let Some(name) = self.field.as_deref() else {
            return Contour::Off;
        };
        match self.current(results).field(name) {
            Some(field) => {
                let (lo, hi) = results::contour_range(field);
                Contour::On { field, lo, hi }
            }
            None => Contour::Missing(name),
        }
    }

    /// The vertex data for the GPU layer.
    pub fn geometry(&self, results: &Results, version: u64) -> Geometry {
        let mesh = &results.mesh;
        let step = self.current(results);
        let scale = if self.deformed { self.scale } else { 0.0 };
        let positions = results::deformed_positions(mesh, step, scale);
        let values = match self.contour(results) {
            Contour::On { field, lo, hi } => results::triangle_values(mesh, field, lo, hi),
            _ => Vec::new(),
        };
        let faces = face_vertices(mesh, &positions, &values);
        Geometry {
            version,
            positions,
            faces,
        }
    }
}

/// The contour field names of all steps, in the order of the first step that has each one.
pub fn field_names(results: &Results) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for step in &results.steps {
        for f in &step.fields {
            if !names.contains(&f.name) {
                names.push(f.name.clone());
            }
        }
    }
    names
}

/// A value with 4 significant digits.
pub fn format_value(v: f64) -> String {
    format_sig(v, 4)
}

/// A value with `sig` significant digits.
fn format_sig(v: f64, sig: usize) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    // Round first, then choose the fixed or the exponent form.
    let sci = format!("{v:.prec$e}", prec = sig - 1);
    let exponent: i32 = sci
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    if (-3..5).contains(&exponent) {
        let rounded: f64 = sci.parse().unwrap_or(v);
        let digits = (sig as i32 - 1 - exponent).max(0) as usize;
        format!("{rounded:.digits$}")
    } else {
        sci
    }
}

/// The texts of the band limits. Each has 4 significant digits, or more when 4 digits make
/// two different limits equal text.
pub fn format_limits(limits: &[f64]) -> Vec<String> {
    let mut texts = Vec::new();
    for sig in 4..=17 {
        texts = limits.iter().map(|&v| format_sig(v, sig)).collect();
        if texts.windows(2).all(|w| w[0] != w[1]) {
            break;
        }
    }
    texts
}

/// The step label of the step selector: the number, the time and the file.
pub fn step_label(index: usize, step: &Step) -> String {
    format!(
        "Step {} (time {}, {})",
        index + 1,
        format_value(step.time),
        step.file
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> Results {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        crate::load_run(&dir.join("../../fixtures/results/cantilever-2step")).unwrap()
    }

    /// The first view state of a run with no view.
    fn state(r: &Results) -> ViewState {
        let view = View::default();
        ViewState::open(r, &view, &Panel::of(r, &view, false))
    }

    #[test]
    fn new_state_shows_the_last_step_deformed_with_mises() {
        let r = fixture();
        let v = state(&r);
        assert_eq!(v.step, 1);
        assert!(v.deformed && v.ghost);
        assert_eq!(v.scale, results::auto_scale(&r));
        assert_eq!(v.field.as_deref(), Some("MISES"));
    }

    /// The light field `Ez` of a Meep run is no field of `DEFAULT_FIELDS`. The run opens with it,
    /// not with no contour.
    #[test]
    fn a_run_with_no_default_field_opens_with_its_first_field() {
        let mut r = fixture();
        for s in &mut r.steps {
            s.displacement.fill([0.0; 3]);
            s.fields.retain(|f| f.name == "MISES");
            s.fields[0].name = "Ez".into();
        }
        assert_eq!(state(&r).field.as_deref(), Some("Ez"));
    }

    /// A field run with no displacement (light, sound, heat) shows no deformation controls, also
    /// when the view asks for them: they can only show "Largest displacement: 0".
    #[test]
    fn a_run_with_no_displacement_shows_no_deformation_controls() {
        let mut r = fixture();
        for s in &mut r.steps {
            s.displacement.fill([0.0; 3]);
        }
        assert_eq!(
            Panel::of(&r, &View::default(), false).controls,
            [Control::Steps]
        );
        let view = View::parse("controls=deformation,loads").unwrap();
        assert_eq!(Panel::of(&r, &view, true).controls, [Control::Loads]);
    }

    /// A view with no deformation control and no scale shows the true shape, not a scaled shape
    /// with no sign of its scale.
    #[test]
    fn a_view_with_no_deformation_control_shows_the_true_shape() {
        let r = fixture();
        let view = View::parse("fields=MISES\ncontrols=loads").unwrap();
        let v = ViewState::open(&r, &view, &Panel::of(&r, &view, true));
        assert_eq!((v.deformed, v.scale), (true, 1.0));
        let v = state(&r);
        assert_eq!((v.deformed, v.scale), (true, results::auto_scale(&r)));
    }

    /// The view of the agent gives the fields, the controls, the first step, the scale and the
    /// box of the camera, in the text of the `data-` attributes of a report.
    #[test]
    fn a_view_sets_the_first_view() {
        let r = fixture();
        let text = "fields=U3, MISES\ncontrols=\nstep=1\nscale=0\nbox=2,0,0,0,1,1";
        let view = View::parse(text).unwrap();
        let panel = Panel::of(&r, &view, true);
        assert_eq!(panel.controls, []);
        assert_eq!(panel.fields, ["U3", "MISES"]);
        let v = ViewState::open(&r, &view, &panel);
        assert_eq!(
            (v.step, v.field.as_deref(), v.deformed),
            (0, Some("U3"), false)
        );
        let frame = view.frame.unwrap();
        assert_eq!((frame.min, frame.max), ([0.0; 3], [2.0, 1.0, 1.0]));
        for wrong in ["controls=steps,flow", "step=0", "box=1,2,3", "colour=red"] {
            assert!(View::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_step_selector_changes_the_shape_and_the_range() {
        let r = fixture();
        let mut v = state(&r);
        v.field = Some("U3".into());
        let last = v.geometry(&r, 1);
        let Contour::On { lo: lo2, .. } = v.contour(&r) else {
            panic!("U3 is a field");
        };
        v.step = 0;
        let first = v.geometry(&r, 2);
        let Contour::On { lo: lo1, hi, .. } = v.contour(&r) else {
            panic!("U3 is a field");
        };
        // The fixed end does not move, so the highest U3 is 0 in both steps.
        assert_eq!(hi, 0.0);
        assert!((lo2 / lo1 - 2.0).abs() < 1e-9, "{lo1} {lo2}");
        assert_ne!(first.positions, last.positions);
        assert_eq!(first.version, 2);
        // The geometry uses the selected step: the moved positions are x + scale * U.
        let want = results::deformed_positions(&r.mesh, &r.steps[0], v.scale);
        assert_eq!(first.positions, want);
    }

    #[test]
    fn a_playing_animation_shows_each_step_in_turn() {
        let r = fixture();
        let mut v = state(&r);
        assert_eq!(v.play(10.0, 2), None, "the steps do not play at first");
        v.playing = Some(10.0);
        let wait = v.play(10.1, 2).unwrap();
        assert!((wait - (PLAY_SECONDS - 0.1)).abs() < 1e-9, "{wait}");
        assert_eq!(v.step, 1, "a step shows for PLAY_SECONDS");
        assert_eq!(v.play(10.0 + PLAY_SECONDS, 2), Some(PLAY_SECONDS));
        assert_eq!(
            (v.step, v.playing),
            (0, Some(10.0 + PLAY_SECONDS)),
            "the first step comes after the last"
        );
        v.play(10.0 + 2.0 * PLAY_SECONDS, 2);
        assert_eq!(v.step, 1);
    }

    #[test]
    fn face_vertices_carry_the_contour_value_of_each_corner() {
        let r = fixture();
        let v = state(&r);
        let g = v.geometry(&r, 1);
        let mises = r.steps[1].field("MISES").unwrap();
        let (lo, hi) = results::contour_range(mises);
        let values = results::triangle_values(&r.mesh, mises, lo, hi);
        for (i, f) in g.faces.iter().enumerate() {
            let p = r.mesh.triangles[i / 3][i % 3] as usize;
            assert_eq!([f[0], f[1], f[2]], g.positions[p]);
            assert_eq!(f[3], values[i]);
        }
    }

    #[test]
    fn values_have_four_significant_digits() {
        assert_eq!(format_value(0.0), "0");
        assert_eq!(format_value(100.0), "100.0");
        assert_eq!(format_value(1.0), "1.000");
        assert_eq!(format_value(-0.03685902950909793), "-0.03686");
        assert_eq!(format_value(3.4641016151354185), "3.464");
        assert_eq!(format_value(1.0e8), "1.000e8");
        assert_eq!(format_value(4.7619047619e-5), "4.762e-5");
        // Round first, then choose the form.
        assert_eq!(format_value(99.99999999), "100.0");
        assert_eq!(format_value(99999.9999), "1.000e5");
        assert_eq!(format_value(12345.6), "12350");
        assert_eq!(format_value(-12345.6), "-12350");
        assert_eq!(format_value(0.00099999999), "0.001000");
        let r = fixture();
        assert_eq!(
            step_label(1, &r.steps[1]),
            "Step 2 (time 2.000, step-2.vtu)"
        );
    }
}
