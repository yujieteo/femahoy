//! The result view state: the selected step, the deformed shape and the contour field. It
//! makes the vertex data for the GPU layer. Plain functions with no GPU types.

use crate::gpu::{face_vertices, Geometry};
use crate::results::{self, Results, ScalarField, Step};

/// The field that the contour shows first, when the run has it.
const DEFAULT_FIELDS: [&str; 3] = ["MISES", "U magnitude", "NT"];

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
    /// The last step, deformed with the automatic scale, with the von Mises stress (or the
    /// displacement magnitude, or the temperature) as contour.
    pub fn new(results: &Results) -> Self {
        let names = field_names(results);
        let field = DEFAULT_FIELDS
            .iter()
            .find(|d| names.iter().any(|n| n == *d))
            .map(|d| (*d).to_owned());
        ViewState {
            step: results.steps.len().saturating_sub(1),
            deformed: true,
            scale: results::auto_scale(results),
            ghost: true,
            field,
        }
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

    #[test]
    fn new_state_shows_the_last_step_deformed_with_mises() {
        let r = fixture();
        let v = ViewState::new(&r);
        assert_eq!(v.step, 1);
        assert!(v.deformed && v.ghost);
        assert_eq!(v.scale, results::auto_scale(&r));
        assert_eq!(v.field.as_deref(), Some("MISES"));
    }

    #[test]
    fn a_heat_result_shows_the_temperature() {
        let mut r = fixture();
        for step in &mut r.steps {
            let n = step.displacement.len();
            step.fields = vec![results::ScalarField {
                name: "NT".into(),
                quantity: results::Quantity::Other,
                location: results::Location::Point,
                values: vec![20.0; n],
            }];
        }
        assert_eq!(ViewState::new(&r).field.as_deref(), Some("NT"));
    }

    #[test]
    fn the_step_selector_changes_the_shape_and_the_range() {
        let r = fixture();
        let mut v = ViewState::new(&r);
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
    fn the_undeformed_shape_and_plain_faces() {
        let r = fixture();
        let mut v = ViewState::new(&r);
        v.deformed = false;
        v.field = None;
        let g = v.geometry(&r, 1);
        assert_eq!(g.positions, r.mesh.positions);
        assert_eq!(g.faces.len(), 3 * r.mesh.triangles.len());
        assert!(g.faces.iter().all(|f| f[3] == 0.0));
        assert_eq!(v.contour(&r), Contour::Off);
    }

    #[test]
    fn face_vertices_carry_the_contour_value_of_each_corner() {
        let r = fixture();
        let v = ViewState::new(&r);
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
    fn a_field_missing_in_the_step_is_reported() {
        let r = fixture();
        let mut v = ViewState::new(&r);
        v.field = Some("PEEQ".into());
        assert_eq!(v.contour(&r), Contour::Missing("PEEQ"));
        // The faces are drawn plain.
        assert!(v.geometry(&r, 1).faces.iter().all(|f| f[3] == 0.0));
    }

    #[test]
    fn field_names_list_each_field_once() {
        let names = field_names(&fixture());
        assert_eq!(names.iter().filter(|n| *n == "MISES").count(), 1);
        assert_eq!(names[0], "U magnitude");
    }

    #[test]
    fn limits_are_distinct_text() {
        let limits = results::band_limits(100.0 - 4e-6, 100.0 + 4e-6);
        let texts = format_limits(&limits);
        assert_eq!(texts.len(), limits.len());
        assert!(texts.windows(2).all(|w| w[0] != w[1]), "{texts:?}");
        let plain = format_limits(&results::band_limits(0.0, 9.0));
        assert_eq!(plain[1], "1.000");
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
