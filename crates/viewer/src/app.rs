//! The eframe app of the model view on the page of `fh serve`: a viewport, a results panel,
//! a status bar and the mouse controls. The page around it (`crates/fh/src/daemon/page.html`)
//! has the chat and the list of the runs. Before a solve, the
//! viewport shows the model with its supports and loads: the faces of each set in a colour,
//! an arrow for each load and a label for each set.
//! The results show the same labels and arrows, with the outline of the faces of each set.
//! The results panel shows the controls of the view of the run (see [`view::Panel`]), and the
//! viewport shows the marks of the view. A run with no view shows each control that applies, its
//! important fields and the switch "Show all controls". A report can ask for all the controls of the run, and for a step
//! of the open run (see [`Request`]).

use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu;

use crate::camera::{self, Camera};
use crate::gpu::{FrameParams, MeshCallback, MeshGpu};
use crate::mesh::{Bounds, RenderMesh};
use crate::preview::{Kind, Preview, COLORS};
use crate::results::{self, Results, BANDS};
use crate::view::{self, face_vertices, Contour, Control, Geometry, Mark, Panel, View, ViewState};
use crate::widgets::{self, hex, legend, readout, rgb, segmented, swatch, text_row};

/// The text of the viewport when no run is open.
const NO_RUN: &str = "No run is open. Select a run in Runs, or ask the agent for a model.";

/// What the viewer shows.
pub enum Content {
    /// The results of a run, the supports and loads of its input or why there are none, and
    /// the view of the report.
    Results(Box<Results>, Result<Box<Preview>, String>, View),
    /// The model of a solver input with its supports and loads, before the solve.
    Preview(Box<Preview>),
    /// A text in place of the results, for example while they load.
    Text(String),
    /// The results cannot be shown. The text says why.
    Failed(String),
}

/// The cameras share a pose. A gesture asks both canvases for a frame.
pub struct LinkedCamera {
    pub camera: Option<Camera>,
    pub contexts: [egui::Context; 2],
}
pub type CameraLink = Arc<Mutex<LinkedCamera>>;

/// What the page asks the viewer to show. The app takes each part in its next frame.
#[derive(Default)]
pub struct Request {
    /// The new content, for example after an HTTP fetch in the browser.
    pub content: Option<Content>,
    /// The step (from 0) of the open run. It waits while a run loads: it is for that run.
    pub step: Option<usize>,
    /// Show all the controls of a run, also the controls that its view does not show.
    pub all: Option<bool>,
    pub camera: Option<String>,
    pub mesh: Option<bool>,
    pub camera_link: Option<Option<CameraLink>>,
    pub status: String,
}

/// A request that the page fills, then asks for a repaint.
pub type RequestSlot = Arc<Mutex<Request>>;

/// The results of a run with their view state, their panel and their view, or a model before
/// the solve.
enum Body {
    Results(Box<Results>, Box<ViewState>, Loads, Panel, View),
    Preview(Box<Preview>),
}

/// The supports and loads of the input of a run.
struct Loads {
    preview: Result<Box<Preview>, String>,
    /// For each mesh of the results: the mesh is the mesh of the input, so the labels move with
    /// the deformed shape.
    same_mesh: Vec<bool>,
}

struct Shown {
    body: Body,
    geometry: Option<Arc<Geometry>>,
    version: u64,
    camera: Camera,
    home: Camera,
    /// The camera after the last view action. Orbit, pan or zoom away from it shows Custom.
    named: Camera,
    initial_fit: bool,
    /// The index of the mesh on the GPU.
    uploaded: Option<usize>,
}

pub struct ViewerApp {
    canvas_id: String,
    render_state: Option<egui_wgpu::RenderState>,
    shown: Option<Shown>,
    message: Option<(bool, String)>,
    /// The page fills the slot when it opens a run, then asks for a repaint.
    slot: RequestSlot,
    /// Show the labels and the arrows of the supports and loads on the model.
    labels: bool,
    /// Show all the controls of a run, also the controls that its view does not show.
    all: bool,
    /// The page has its own switch "Show all controls", so the results panel shows none.
    page_switch: bool,
    camera_action: Option<String>,
    mesh_only: bool,
    camera_link: Option<CameraLink>,
    /// The scale of the panel text at the last style. A new Dynamic Type size changes it.
    text_scale: f32,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, slot: RequestSlot, canvas_id: String) -> Self {
        Self {
            canvas_id,
            text_scale: 0.0,
            render_state: cc.wgpu_render_state.clone(),
            shown: None,
            message: Some((false, NO_RUN.into())),
            slot,
            labels: true,
            all: false,
            page_switch: false,
            camera_action: None,
            mesh_only: false,
            camera_link: None,
        }
    }

    fn set_content(&mut self, content: Content) {
        let mut frame = None;
        let body = match content {
            Content::Results(r, preview, view) => {
                let state = Box::new(ViewState::open(
                    &r,
                    &view,
                    &panel(&r, &view, &preview, false),
                ));
                let panel = panel(&r, &view, &preview, self.all);
                let same_mesh = (r.meshes.iter())
                    .map(|m| preview.as_ref().is_ok_and(|p| p.matches(m)))
                    .collect();
                frame = view.frame.map(|b| (b, plane(&r, b)));
                Body::Results(r, state, Loads { preview, same_mesh }, panel, view)
            }
            Content::Preview(p) => Body::Preview(p),
            Content::Text(text) => {
                self.message = Some((false, text));
                return;
            }
            Content::Failed(text) => {
                self.message = Some((true, text));
                self.shown = None;
                return;
            }
        };
        let mut shown = Shown {
            body,
            geometry: None,
            version: 0,
            camera: Camera::fit(None, false),
            home: Camera::fit(None, false),
            named: Camera::fit(None, false),
            uploaded: None,
            initial_fit: true,
        };
        shown.home = match frame {
            _ if self.mesh_only => Camera::fit(shown.mesh().bounds, false),
            Some((b, top)) => Camera::fit(Some(b), top),
            None => Camera::fit(shown.view_bounds(), false),
        };
        shown.camera = shown.home;
        shown.named = shown.home;
        self.message = None;
        shown.refresh();
        self.shown = Some(shown);
    }

    /// Take the parts of the request of the page.
    fn take_request(&mut self) {
        let slot = self.slot.clone();
        let Ok(mut request) = slot.lock() else {
            return;
        };
        if let Some(mesh) = request.mesh.take() {
            self.mesh_only = mesh;
        }
        if let Some(link) = request.camera_link.take() {
            self.camera_link = link;
        }
        if let Some(content) = request.content.take() {
            self.set_content(content);
        }
        if let Some(all) = request.all.take() {
            self.all = all;
            self.page_switch = true;
            if let Some(Body::Results(r, _, loads, p, view)) =
                self.shown.as_mut().map(|s| &mut s.body)
            {
                *p = panel(r, view, &loads.preview, all);
            }
        }
        if let Some(action) = request.camera.take() {
            self.camera_action = Some(action);
        }
        if self.message.is_some() {
            return;
        }
        let Some(step) = request.step.take() else {
            return;
        };
        if let Some(shown) = self.shown.as_mut() {
            if let Body::Results(r, v, ..) = &mut shown.body {
                v.step = step.min(r.steps.len().saturating_sub(1));
                v.probe = None;
                v.playing = None;
                shown.refresh();
            }
        }
    }

    /// Put the mesh of the shown step on the GPU when it is not there. A step of a staged model
    /// can have another mesh than the step before it.
    fn upload(&mut self) {
        let (Some(rs), Some(shown)) = (&self.render_state, &mut self.shown) else {
            return;
        };
        let index = shown.mesh_index();
        if shown.uploaded != Some(index) {
            let gpu = MeshGpu::new(rs, shown.mesh());
            rs.renderer.write().callback_resources.insert(gpu);
            shown.uploaded = Some(index);
        }
    }

    fn status_text(&self) -> String {
        let Some(s) = &self.shown else {
            return String::new();
        };
        let m = s.mesh();
        let mut text = match &s.body {
            Body::Preview(p) => format!("{} before the solve: ", p.title),
            Body::Results(..) => String::new(),
        };
        text.push_str(&format!(
            "{} nodes, {} elements, {} surface triangles",
            m.node_count,
            m.element_count,
            m.triangles.len()
        ));
        if m.skipped_elements > 0 {
            text.push_str(&format!(
                ", {} elements not drawn (unsupported type or missing node)",
                m.skipped_elements
            ));
        }
        text
    }
}

impl Shown {
    /// The bounds of the mesh, of the moved points and of the arrows of the loads, for the first view.
    fn view_bounds(&self) -> Option<Bounds> {
        let (mut b, p) = match &self.body {
            Body::Preview(p) => (p.mesh.bounds?, p),
            Body::Results(r, _, Loads { preview: Ok(p), .. }, ..) => (results::view_bounds(r)?, p),
            Body::Results(r, ..) => return results::view_bounds(r),
        };
        for (tail, head) in p.items.iter().filter_map(|i| p.arrow(i)) {
            for x in [tail, head] {
                b.min = std::array::from_fn(|k| b.min[k].min(x[k]));
                b.max = std::array::from_fn(|k| b.max[k].max(x[k]));
            }
        }
        Some(b)
    }

    /// The mesh of the shown step.
    fn mesh(&self) -> &RenderMesh {
        match &self.body {
            Body::Results(r, v, ..) => r.mesh(v.current(r)),
            Body::Preview(p) => &p.mesh,
        }
    }

    /// The index of the mesh of the shown step. A preview has 1 mesh.
    fn mesh_index(&self) -> usize {
        match &self.body {
            Body::Results(r, v, ..) => v.current(r).mesh,
            Body::Preview(_) => 0,
        }
    }

    /// Make new vertex data after a change of the view state.
    fn refresh(&mut self) {
        self.version += 1;
        let geometry = match &self.body {
            Body::Results(r, v, ..) => v.geometry(r, self.version),
            Body::Preview(p) => Geometry {
                version: self.version,
                positions: Vec::new(),
                faces: face_vertices(&p.mesh, &p.mesh.positions, &p.face_values()),
            },
        };
        self.geometry = Some(Arc::new(geometry));
    }

    /// Include the visible deformation scale when Fit frames the model.
    fn fit_bounds(&self) -> Option<Bounds> {
        let mut bounds = self.view_bounds()?;
        if let Some(geometry) = &self.geometry {
            for point in &geometry.positions {
                for (k, value) in point.iter().enumerate() {
                    bounds.min[k] = bounds.min[k].min(*value);
                    bounds.max[k] = bounds.max[k].max(*value);
                }
            }
        }
        Some(bounds)
    }

    /// True when the panel shows the group of controls `c`. A preview has no such group.
    fn shows(&self, c: Control) -> bool {
        match &self.body {
            Body::Results(_, _, _, panel, _) => panel.controls.contains(&c),
            Body::Preview(_) => false,
        }
    }

    /// The draw parameters of the body: the contour, the colours and the ghost outline. The ghost
    /// outline shows only with the deformation controls.
    fn frame(&self) -> (bool, Option<[[f32; 3]; BANDS]>, bool) {
        let ghost = self.shows(Control::Deformation);
        match &self.body {
            Body::Results(r, v, ..) => (
                matches!(v.contour(r), Contour::On { .. }),
                None,
                ghost && v.deformed && v.ghost,
            ),
            Body::Preview(_) => (true, Some(COLORS), false),
        }
    }

    /// The panel of the body. A run with no view has the switch `all` when it is given. Returns
    /// true when the view state changed.
    fn panel(&mut self, ui: &mut egui::Ui, labels: &mut bool, all: Option<&mut bool>) -> bool {
        let drawn = self.geometry.as_deref().map(|g| &g.positions[..]);
        match &mut self.body {
            Body::Results(r, v, loads, panel, view) => {
                let mut switch = all.filter(|_| *view == View::default());
                let before = switch.as_deref().copied();
                let changed = results_panel(ui, r, v, panel, switch.as_deref_mut(), drawn);
                if let Some(all) = switch.filter(|a| Some(**a) != before) {
                    *panel = self::panel(r, view, &loads.preview, *all);
                }
                if panel.controls.contains(&Control::Loads) {
                    let same_mesh = loads.same_mesh[v.current(r).mesh];
                    loads_panel(ui, loads, same_mesh, labels);
                }
                changed
            }
            Body::Preview(p) => {
                preview_panel(ui, p, labels);
                false
            }
        }
    }

    /// The supports and loads on top of the model, with the positions of the drawn mesh. The
    /// results show them only with the supports and loads controls.
    fn overlay(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        m: &camera::Mat4,
        placed: &mut Vec<egui::Rect>,
    ) {
        let eye = self.camera.eye();
        match &self.body {
            Body::Preview(p) => {
                loads_overlay(painter, rect, m, eye, p, &p.mesh.positions, false, placed)
            }
            Body::Results(r, v, loads, ..) => {
                if !self.shows(Control::Loads) {
                    return;
                }
                let Ok(p) = &loads.preview else { return };
                let positions = match (&self.geometry, loads.same_mesh[v.current(r).mesh]) {
                    (Some(g), true) => &g.positions,
                    _ => &p.mesh.positions,
                };
                loads_overlay(painter, rect, m, eye, p, positions, true, placed);
            }
        }
    }

    /// The marks of the view at the shown step.
    fn marks(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        m: &camera::Mat4,
        placed: &mut Vec<egui::Rect>,
    ) {
        let Body::Results(_, v, _, _, view) = &self.body else {
            return;
        };
        let shown = |mark: &&Mark| mark.step.is_none_or(|s| s == v.step);
        if let Some(probe) = &v.probe {
            if let Some(at) = probe.drawn_at.and_then(|p| screen(m, rect, p)) {
                painter.circle(
                    at,
                    6.0,
                    hex(MARK),
                    egui::Stroke::new(2.0, painter.ctx().global_style().visuals.text_color()),
                );
                tag(
                    painter,
                    rect,
                    egui::Rect::from_pos(at),
                    &format!("{}: {}", probe.label, probe.value),
                    hex(MARK),
                    placed,
                );
            }
        }
        for mark in view.marks.iter().filter(shown) {
            let Some(at) = screen(m, rect, mark.at) else {
                continue;
            };
            painter.circle(
                at,
                5.0,
                hex(MARK),
                egui::Stroke::new(2.0, egui::Color32::BLACK),
            );
            tag(
                painter,
                rect,
                egui::Rect::from_pos(at),
                &mark.text,
                hex(MARK),
                placed,
            );
        }
    }
}

/// The colour of a mark: the accent colour of the light appearance of the page.
const MARK: u32 = crate::ui::LIGHT.accent;

/// The place of the point `x` on the view `rect` with the matrix `m`, or `None` behind the eye.
fn screen(m: &camera::Mat4, rect: egui::Rect, x: [f32; 3]) -> Option<egui::Pos2> {
    let c = camera::transform(m, x);
    (c[3] > 1e-6).then(|| {
        let (x, y) = (c[0] / c[3], c[1] / c[3]);
        egui::pos2(
            rect.left() + (x + 1.0) * 0.5 * rect.width(),
            rect.top() + (1.0 - y) * 0.5 * rect.height(),
        )
    })
}

/// The text `text` on a box of the colour `color`, next to the screen box `around` (a point or
/// the faces of a set) on the side away from the view centre, in the view `rect`, with a line
/// width of at most half the view. The tag does not cover the earlier tags in `placed`, if the
/// view has space for it, and is added to `placed`.
fn tag(
    painter: &egui::Painter,
    rect: egui::Rect,
    around: egui::Rect,
    text: &str,
    color: egui::Color32,
    placed: &mut Vec<egui::Rect>,
) {
    let [r, g, b] = [color.r(), color.g(), color.b()].map(|x| f32::from(x) / 255.0);
    let ink = if 0.3 * r + 0.6 * g + 0.1 * b > 0.5 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    };
    let font = egui::FontId::proportional(14.0);
    // Wrap long text at half the view width, so that on a phone a tag does not cover the model.
    let wrap = (0.5 * rect.width()).max(180.0);
    let galley = painter.layout(text.to_owned(), font, ink, wrap);
    // Keep the text in the view.
    let inner = rect.shrink(4.0);
    let fit = |place: egui::Rect| {
        let dx = (inner.right() - place.right())
            .min(0.0)
            .max(inner.left() - place.left());
        let dy = (inner.bottom() - place.bottom())
            .min(0.0)
            .max(inner.top() - place.top());
        place.translate(egui::vec2(dx, dy))
    };
    // The camera looks at the view centre, so a tag on the far side of the point from the
    // centre does not cover the model.
    let at = around.center();
    let (left, up) = (at.x < rect.center().x, at.y < rect.center().y);
    let anchor = egui::Align2([
        [egui::Align::Min, egui::Align::Max][usize::from(left)],
        [egui::Align::Min, egui::Align::Max][usize::from(up)],
    ]);
    // Above or below the box, so that the tag does not cover the faces of a set.
    let y = if up {
        around.top() - 6.0
    } else {
        around.bottom() + 6.0
    };
    let x = if left { at.x - 6.0 } else { at.x + 6.0 };
    let first = fit(anchor.anchor_size(egui::pos2(x, y), galley.size()));
    let mut place = first;
    // Move the tag below each earlier tag that it covers.
    for _ in 0..placed.len() {
        let Some(other) = placed
            .iter()
            .find(|o| o.expand(1.0).intersects(place.expand(3.0)))
        else {
            break;
        };
        place = fit(place.translate(egui::vec2(0.0, other.bottom() + 5.0 - place.top())));
    }
    placed.push(place.expand(3.0));
    if place != first {
        // A line from the point to the moved tag.
        painter.line_segment(
            [at, place.expand(3.0).clamp(at)],
            egui::Stroke::new(1.5, color),
        );
    }
    painter.rect_filled(place.expand(3.0), 3.0, color);
    painter.galley(place.min, galley, ink);
}

/// The panel of the results `r` with the view `view` (the important fields when it is empty),
/// or with each control of the run when `all` is true. `preview` has the supports and loads of
/// the input.
fn panel(r: &Results, view: &View, preview: &Result<Box<Preview>, String>, all: bool) -> Panel {
    let loads = preview.as_ref().is_ok_and(|p| !p.items.is_empty());
    let each = View::default();
    let view = if all {
        Some(&each)
    } else {
        Some(view).filter(|v| **v != each)
    };
    Panel::of(r, view, loads)
}

/// True for a plane model, which shows from the top: the box `b` of the view is flat (its z
/// size is at most 1/20 of its x or y size), and no node of the run `r` moves in z.
fn plane(r: &Results, b: Bounds) -> bool {
    let size: [f32; 3] = std::array::from_fn(|k| b.max[k] - b.min[k]);
    let still = r
        .steps
        .iter()
        .all(|s| s.displacement.iter().all(|u| u[2] == 0.0));
    still && size[2] <= size[0].max(size[1]) / 20.0
}

fn kind_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Support => "support",
        Kind::Load => "load",
    }
}

/// The supports and loads of a run under its results: a switch and the label of each set.
/// `same_mesh`: the mesh of the shown step is the mesh of the input.
fn loads_panel(ui: &mut egui::Ui, loads: &Loads, same_mesh: bool, labels: &mut bool) {
    ui.separator();
    ui.strong("Supports and loads");
    let p = match &loads.preview {
        Ok(p) => p,
        Err(e) => {
            ui.weak(format!("Not available: {e}"));
            return;
        }
    };
    widgets::checkbox(ui, labels, "Show them on the model");
    for item in &p.items {
        text_row(ui, |ui| {
            swatch(ui, COLORS[item.color]);
            ui.label(&item.label);
        });
    }
    if !same_mesh {
        ui.weak("They show on the undeformed shape: the results mesh is not the input mesh.");
    }
}

/// The supports and loads with their colours and their texts.
fn preview_panel(ui: &mut egui::Ui, p: &Preview, labels: &mut bool) {
    ui.heading("Supports and loads");
    ui.label(format!(
        "{}: the input of the solve, before the solve.",
        p.title
    ));
    ui.label(format!("Units: {}", p.units));
    widgets::checkbox(ui, labels, "Show the labels and the arrows");
    if p.items.is_empty() {
        let error = ui.visuals().error_fg_color;
        ui.colored_label(error, "The input has no support and no load.");
    }
    for item in &p.items {
        ui.separator();
        text_row(ui, |ui| {
            swatch(ui, COLORS[item.color]);
            ui.strong(&item.label);
            ui.weak(kind_text(item.kind));
        });
        for line in p.describe(item) {
            ui.label(line);
        }
        if item.triangles.is_empty() {
            let error = ui.visuals().error_fg_color;
            ui.colored_label(error, "No surface face has all of its nodes in this set.");
        }
    }
    ui.separator();
    ui.weak("Each arrow shows the direction of the total force of a load.");
}

/// The supports and loads on top of the model: with `outline`, the outline of the faces of
/// each set, then an arrow for each load and a label for each set. `positions` are the
/// positions of the mesh of the preview as drawn, for example deformed.
fn loads_overlay(
    painter: &egui::Painter,
    rect: egui::Rect,
    m: &camera::Mat4,
    eye: [f32; 3],
    p: &Preview,
    positions: &[[f32; 3]],
    outline: bool,
    placed: &mut Vec<egui::Rect>,
) {
    let screen = |x: [f32; 3]| screen(m, rect, x);
    for item in p.items.iter().filter(|_| outline) {
        let color = rgb(COLORS[item.color]);
        for &([a, b], normal) in &item.outline {
            let (Some(&pa), Some(&pb)) = (positions.get(a as usize), positions.get(b as usize))
            else {
                continue;
            };
            let (Some(sa), Some(sb)) = (screen(pa), screen(pb)) else {
                continue;
            };
            // An edge of a face that looks away from the eye is behind the part: a thin line.
            let front = (0..3).map(|k| normal[k] * (eye[k] - pa[k])).sum::<f32>() > 0.0;
            if front {
                painter.line_segment([sa, sb], egui::Stroke::new(4.0, egui::Color32::BLACK));
                painter.line_segment([sa, sb], egui::Stroke::new(2.5, color));
            } else {
                painter.line_segment([sa, sb], egui::Stroke::new(1.0, color.gamma_multiply(0.5)));
            }
        }
    }
    for item in &p.items {
        let color = rgb(COLORS[item.color]);
        let shift = p.shift(item, positions);
        let moved = |x: [f32; 3]| screen(std::array::from_fn(|k| x[k] + shift[k]));
        let mut at = moved(item.center.map(|x| x as f32));
        if let Some((tail, head)) = p.arrow(item) {
            if let (Some(a), Some(b)) = (moved(tail), moved(head)) {
                painter.arrow(a, b - a, egui::Stroke::new(5.0, egui::Color32::BLACK));
                painter.arrow(a, b - a, egui::Stroke::new(3.0, color));
                at = Some(a);
            }
        }
        let Some(at) = at else { continue };
        // A support tag goes next to the screen box of its outline, not on its faces.
        let mut around = egui::Rect::from_pos(at);
        if p.arrow(item).is_none() {
            for &([a, b], _) in &item.outline {
                for x in [a, b].iter().filter_map(|&n| positions.get(n as usize)) {
                    if let Some(s) = screen(*x) {
                        around.extend_with(s);
                    }
                }
            }
        }
        tag(
            painter,
            rect,
            around,
            &item.label,
            rgb(COLORS[item.color]),
            placed,
        );
    }
}

/// The results panel: the switch `all` when it is given, and the controls and the fields of
/// `panel`. `drawn` has the positions of the drawn mesh of `v`. Returns true when the view
/// state changed.
fn results_panel(
    ui: &mut egui::Ui,
    r: &Results,
    v: &mut ViewState,
    panel: &Panel,
    mut all: Option<&mut bool>,
    drawn: Option<&[[f32; 3]]>,
) -> bool {
    let before = v.clone();
    // One wrapped row, so that a wide short panel (a phone) keeps its height for the legend.
    let mut header = |ui: &mut egui::Ui| {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Results");
            ui.label(format!("Units: {}", r.units));
            if let Some(all) = all.as_deref_mut() {
                widgets::checkbox(ui, all, "Show all controls")
                    .on_hover_text("also the fields that are not important for this run");
            }
        });
    };
    // A short panel (the panel below the model on a phone) shows the header last, so that
    // the colour scale and the probe stay in view at a large text size.
    let short = ui.clip_rect().height() < 400.0;
    if !short {
        header(ui);
        ui.separator();
    }

    let n = r.steps.len();
    // Before the controls, so that they show the step of the shape.
    if let Some(wait) = v.play(ui.input(|i| i.time), n) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    let shows = |c: Control| panel.controls.contains(&c);
    if shows(Control::Steps) {
        steps_controls(ui, r, v);
    }
    if shows(Control::Deformation) {
        deformation_controls(ui, r, v);
    }

    // The label and its menu on one row, so that a short panel (a phone) keeps a row for the legend.
    // With no menu (a report), the label goes too: the legend title names the field.
    if !panel.fields.is_empty() {
        ui.horizontal(|ui| {
            ui.strong("Contour");
            egui::ComboBox::from_id_salt("field")
                .selected_text(v.field.clone().unwrap_or_else(|| "None".into()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut v.field, None, "None");
                    for name in &panel.fields {
                        ui.selectable_value(&mut v.field, Some(name.clone()), name);
                    }
                });
        });
    }
    // Directly below its menu, so that a short panel (a phone) shows the scale of the colours.
    match v.contour(r) {
        Contour::Off => {}
        Contour::Missing(name) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Step {} has no field {name}.", v.step + 1),
            );
        }
        Contour::On { field, lo, hi } => {
            let quantity = if v.field == v.configured_field {
                v.quantity.as_deref()
            } else {
                None
            };
            legend(
                ui,
                &r.units,
                field,
                lo,
                hi,
                quantity,
                v.field == v.configured_field && v.range.is_some(),
            );
        }
    }
    if v.step != before.step
        || v.field != before.field
        || v.scale != before.scale
        || v.deformed != before.deformed
    {
        v.probe = None;
    }
    if shows(Control::Scale) {
        readout(
            ui,
            "Deformation",
            &format!("×{}", if v.deformed { v.scale } else { 0.0 }),
        );
    }
    if shows(Control::Range) {
        ui.label("Colour range");
        let items = [
            ("Current step", !v.all_steps, "limits of this step"),
            ("All steps", v.all_steps, "one range across this run"),
        ];
        if let Some(i) = segmented(ui, &items) {
            v.all_steps = i == 1;
        }
        if matches!(v.contour(r), Contour::On { .. }) {
            ui.weak(if v.all_steps {
                "Range: all steps"
            } else {
                "Range: current step"
            });
        }
    }
    if shows(Control::Probe) {
        let same =
            v.step == before.step && v.scale == before.scale && v.deformed == before.deformed;
        let scale_shown = shows(Control::Scale) || shows(Control::Deformation);
        probe_controls(ui, r, v, drawn.filter(|_| same), scale_shown);
    } else {
        v.probe_mode = false;
        v.probe = None;
    }
    if short {
        ui.separator();
        header(ui);
    }
    *v != before
}

/// The probe controls. `drawn` has the positions of the drawn mesh, or is `None` when the
/// drawn mesh is not yet the mesh of `v`. The probe shows the deformation scale when
/// `scale_shown` is false.
fn probe_controls(
    ui: &mut egui::Ui,
    r: &Results,
    v: &mut ViewState,
    drawn: Option<&[[f32; 3]]>,
    scale_shown: bool,
) {
    let field = v.field.as_deref().and_then(|n| v.current(r).field(n));
    let mut chosen = None;
    // One wrapped row, so that a wide short panel (a phone) shows the buttons with no scroll.
    ui.horizontal_wrapped(|ui| {
        widgets::checkbox(ui, &mut v.probe_mode, "Probe surface")
            .on_hover_text("click the model to probe, Escape clears");
        if field.is_none() {
            return;
        }
        if widgets::button(ui, "Min", "locate the actual minimum item").clicked() {
            chosen = Some(false);
        }
        if widgets::button(ui, "Max", "locate the actual maximum item").clicked() {
            chosen = Some(true);
        }
        if widgets::button(ui, "Clear", "clear probe (Escape)").clicked() {
            chosen = None;
            v.probe = None;
        }
    });
    let Some(field) = field else {
        return;
    };
    let mesh = r.mesh(v.current(r));
    let positions = |v: &ViewState| match drawn {
        Some(p) => std::borrow::Cow::Borrowed(p),
        None => std::borrow::Cow::Owned(results::deformed_positions(
            mesh,
            v.current(r),
            if v.deformed { v.scale } else { 0.0 },
        )),
    };
    if let Some(max) = chosen {
        v.probe = crate::probe::extreme(mesh, &positions(v), field, max);
    }
    if v.probe_mode && !field.values.is_empty() {
        let mut index = v.probe.as_ref().map_or(1, |p| p.index + 1);
        ui.horizontal(|ui| {
            let text = match field.location {
                results::Location::Point => "Node index",
                results::Location::Cell => "Element index",
            };
            ui.label(text);
            if ui
                .add(
                    egui::DragValue::new(&mut index)
                        .range(1..=field.values.len())
                        .speed(1),
                )
                .changed()
            {
                v.probe = crate::probe::at_index(mesh, &positions(v), field, index - 1);
            }
        });
    }
    if let Some(p) = &v.probe {
        ui.strong(format!(
            "{} · {} · Step {}",
            p.label,
            field.name,
            v.step + 1
        ));
        let location = match field.location {
            results::Location::Point => "At node index",
            results::Location::Cell => "Per element index",
        };
        readout(ui, location, &(p.index + 1).to_string());
        readout(
            ui,
            "Value",
            &format!("{} {}", p.value, view::unit_text(&r.units, field.quantity)),
        );
        if let Some(at) = p.at {
            ui.label(format!(
                "Model coordinates: {}, {}, {} ({})",
                at[0],
                at[1],
                at[2],
                view::unit_text(&r.units, results::Quantity::Displacement)
            ));
            ui.weak("Surface marker may be hidden by another face.");
        } else {
            ui.label(format!("{} is hidden: no surface marker.", p.label));
        }
        if !scale_shown {
            readout(
                ui,
                "Deformation",
                &format!("×{}", if v.deformed { v.scale } else { 0.0 }),
            );
        }
    }
}

/// The step selector, the play button and the step slider.
fn steps_controls(ui: &mut egui::Ui, r: &Results, v: &mut ViewState) {
    let n = r.steps.len();
    ui.strong("Step");
    ui.horizontal(|ui| {
        let back = ui.add_enabled_ui(v.step > 0, |ui| widgets::button(ui, "<", "previous step"));
        if back.inner.clicked() {
            v.step -= 1;
        }
        egui::ComboBox::from_id_salt("step")
            .selected_text(view::step_label(v.step, &r.steps[v.step]))
            .show_ui(ui, |ui| {
                for (i, s) in r.steps.iter().enumerate() {
                    ui.selectable_value(&mut v.step, i, view::step_label(i, s));
                }
            });
        let next = ui.add_enabled_ui(v.step + 1 < n, |ui| widgets::button(ui, ">", "next step"));
        if next.inner.clicked() {
            v.step += 1;
        }
    });
    if n > 1 {
        ui.horizontal(|ui| {
            let text = if v.playing.is_some() { "Pause" } else { "Play" };
            if widgets::button(ui, text, "show each step in turn").clicked() {
                v.playing = match v.playing {
                    Some(_) => None,
                    None => Some(ui.input(|i| i.time)),
                };
            }
            // The rail fills the row, so that each step is a long drag on a phone.
            ui.spacing_mut().slider_width = ui.available_width();
            ui.add(egui::Slider::new(&mut v.step, 0..=n - 1).show_value(false));
        });
    }
    ui.label(format!("{n} step{}", if n == 1 { "" } else { "s" }));
    ui.separator();
}

/// The deformed shape, its scale and the undeformed outline.
fn deformation_controls(ui: &mut egui::Ui, r: &Results, v: &mut ViewState) {
    ui.strong("Deformed shape");
    widgets::checkbox(ui, &mut v.deformed, "Show the deformed shape");
    ui.add_enabled_ui(v.deformed, |ui| {
        ui.horizontal(|ui| {
            ui.label("Scale");
            let speed = v.scale.max(1e-6) * 0.01;
            ui.add(
                egui::DragValue::new(&mut v.scale)
                    .range(0.0..=1e12)
                    .speed(speed)
                    .custom_formatter(|x, _| view::format_value(x)),
            );
            let auto = results::auto_scale(r);
            let items = [
                (
                    "Auto",
                    v.scale == auto,
                    "a scale that shows the deformation",
                ),
                ("1\u{d7}", v.scale == 1.0, "true scale"),
            ];
            match segmented(ui, &items) {
                Some(0) => v.scale = auto,
                Some(_) => v.scale = 1.0,
                None => {}
            }
        });
        widgets::checkbox(ui, &mut v.ghost, "Undeformed outline");
    });
    let step = v.current(r);
    let unit = view::unit_text(&r.units, results::Quantity::Displacement);
    let largest = view::format_value(step.max_displacement());
    readout(ui, "Largest displacement", &format!("{largest} {unit}"));
    ui.separator();
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if crate::controls::fit_backing_store(&self.canvas_id) {
            ui.ctx().request_repaint();
        }
        let scale = crate::controls::text_scale();
        if scale != self.text_scale {
            self.text_scale = scale;
            widgets::style(ui.ctx(), scale);
            ui.ctx().request_repaint();
        }
        self.take_request();
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(Shown {
                body: Body::Results(_, v, ..),
                ..
            }) = self.shown.as_mut()
            {
                v.probe = None;
            }
        }
        if self.mesh_only {
            if let Some(Shown {
                body: Body::Results(_, state, ..),
                ..
            }) = self.shown.as_mut()
            {
                if state.deformed || state.field.is_some() {
                    state.deformed = false;
                    state.field = None;
                    if let Some(shown) = self.shown.as_mut() {
                        shown.refresh();
                    }
                }
            }
        }
        if let Ok(mut request) = self.slot.lock() {
            request.status = self
                .message
                .as_ref()
                .map(|(_, text)| text.clone())
                .unwrap_or_else(|| self.status_text());
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 0.0;
            // 1 row only: at a large text size on a phone, 2 rows take the space of the
            // Results panel. The full text shows on hover and goes to the screen reader.
            ui.add(egui::Label::new(egui::RichText::new(self.status_text()).weak()).truncate());
        });
        if let Some(shown) = self.shown.as_mut().filter(|_| !self.mesh_only) {
            // On a narrow view (a phone) the results go below the model, so that the model
            // keeps the full width.
            let panel = if ui.available_width() < 600.0 {
                egui::Panel::bottom("results").exact_size(ui.available_height() * 0.4)
            } else {
                egui::Panel::right("results").min_size(260.0)
            };
            panel.resizable(false).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let all = (!self.page_switch).then_some(&mut self.all);
                    if shown.panel(ui, &mut self.labels, all) {
                        shown.refresh();
                    }
                });
            });
        }
        self.upload();
        if let Some(shown) = &self.shown {
            let controls = match &shown.body {
                Body::Results(_, _, _, _, view) => view.controls.as_deref().unwrap_or(Control::ALL),
                Body::Preview(_) => Control::ALL,
            };
            let direction = if shown.camera == shown.named {
                shown.camera.direction_name()
            } else {
                "Custom"
            };
            crate::controls::update(&self.canvas_id, controls, self.all, direction);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some((error, text)) = &self.message {
                ui.centered_and_justified(|ui| {
                    if *error {
                        ui.colored_label(ui.visuals().error_fg_color, text);
                    } else {
                        ui.weak(text);
                    }
                });
                return;
            }
            let Some(shown) = self.shown.as_mut() else {
                return;
            };
            let size = ui.available_size();
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            if response.clicked() {
                response.request_focus();
            }
            let aspect = rect.width() / rect.height().max(1.0);
            if shown.initial_fit {
                shown.home.fit_distance(aspect);
                shown.camera = shown.home;
                shown.named = shown.camera;
                shown.initial_fit = false;
                if self.mesh_only {
                    if let Some(link) = &self.camera_link {
                        if let Ok(mut camera) = link.lock() {
                            camera.camera = None;
                        }
                    }
                }
            }
            if let Some(link) = &self.camera_link {
                if let Ok(mut camera) = link.lock() {
                    if let Some(camera) = camera.camera {
                        shown.camera = camera;
                    } else {
                        camera.camera = Some(shown.camera);
                        for ctx in &camera.contexts {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            let before = shown.camera;
            if let Some(action) = self.camera_action.take() {
                ui.ctx().request_repaint();
                match action.as_str() {
                    "fit" => shown.camera.fit_bounds(shown.fit_bounds(), aspect),
                    "report-view" => {
                        shown.camera = shown.home;
                        shown.camera.fit_distance(aspect);
                    }
                    direction => {
                        shown.camera.direction(direction);
                        shown.camera.fit_bounds(shown.fit_bounds(), aspect);
                    }
                }
                shown.named = shown.camera;
            }
            let delta = response.drag_delta();
            // Two fingers pan and pinch. The first finger also drives the pointer, so it must not orbit.
            let touch = ui
                .input(|i| i.multi_touch())
                .filter(|t| rect.contains(t.start_pos));
            let pan = response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || (response.dragged_by(egui::PointerButton::Primary)
                    && ui.input(|i| i.modifiers.shift));
            if let Some(t) = touch {
                shown
                    .camera
                    .pan(t.translation_delta.x, t.translation_delta.y, rect.height());
            } else if pan {
                shown.camera.pan(delta.x, delta.y, rect.height());
            } else if response.dragged_by(egui::PointerButton::Primary) {
                shown.camera.orbit(delta.x, delta.y);
            }
            if response.hovered() || touch.is_some() {
                let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
                shown.camera.zoom(scroll, pinch);
            }
            if response.double_clicked() {
                shown.camera = shown.home;
                shown.camera.fit_distance(aspect);
                shown.named = shown.camera;
                if self.mesh_only {
                    if let Some(link) = &self.camera_link {
                        if let Ok(mut camera) = link.lock() {
                            camera.camera = None;
                            for ctx in &camera.contexts {
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
            if shown.camera != before {
                if let Some(link) = &self.camera_link {
                    if let Ok(mut camera) = link.lock() {
                        camera.camera = Some(shown.camera);
                        for ctx in &camera.contexts {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            let (contour, palette, ghost) = shown.frame();
            let view_proj = shown
                .camera
                .view_proj(rect.width() / rect.height().max(1.0));
            if response.clicked() && !response.double_clicked() {
                if let (Some(pointer), Body::Results(r, v, ..), Some(g)) = (
                    response.interact_pointer_pos(),
                    &mut shown.body,
                    &shown.geometry,
                ) {
                    if v.probe_mode {
                        if let Some(field) = v.field.as_deref().and_then(|n| v.current(r).field(n))
                        {
                            let mesh = r.mesh(v.current(r));
                            let ndc = [
                                2.0 * (pointer.x - rect.left()) / rect.width() - 1.0,
                                1.0 - 2.0 * (pointer.y - rect.top()) / rect.height(),
                            ];
                            let (origin, direction) =
                                shown.camera.ray(rect.width() / rect.height().max(1.0), ndc);
                            v.probe = crate::probe::pick(
                                mesh,
                                &g.positions,
                                field,
                                origin,
                                direction,
                                &view_proj,
                            );
                        }
                    }
                }
            }
            let params = FrameParams {
                view_proj,
                eye: shown.camera.eye(),
                dark: ui.visuals().dark_mode,
                contour,
                ghost,
                palette,
                mesh: match &shown.body {
                    Body::Results(_, v, ..) => v.mesh,
                    Body::Preview(_) => true,
                },
            };
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                MeshCallback {
                    params,
                    geometry: shown.geometry.clone(),
                },
            ));
            let painter = ui.painter_at(rect);
            let mut placed = Vec::new();
            if self.labels && !self.mesh_only {
                shown.overlay(&painter, rect, &view_proj, &mut placed);
            }
            shown.marks(&painter, rect, &view_proj, &mut placed);
        });
    }
}
