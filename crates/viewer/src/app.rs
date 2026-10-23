//! The eframe app of the model view on the page of `fh serve`: a viewport, a results panel,
//! a status bar and the mouse controls. The page around it (`crates/fh/src/daemon/page.html`)
//! has the chat and the list of the runs. Before a solve, the
//! viewport shows the model with its supports and loads: the faces of each set in a colour,
//! an arrow for each load and a label for each set.
//! The results show the same labels and arrows, with the outline of the faces of each set.

use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu;

use crate::camera::{self, Camera};
use crate::gpu::{FrameParams, MeshCallback, MeshGpu};
use crate::mesh::{Bounds, RenderMesh};
use crate::preview::{Kind, Preview, COLORS};
use crate::results::{self, Location, Results, BANDS, BAND_COLORS};
use crate::view::{self, face_vertices, Contour, Geometry, ViewState};

/// The font of the controls: DM Sans (SIL Open Font License 1.1, `fonts/OFL.txt`), the sans
/// font of the page.
const DM_SANS: &[u8] = include_bytes!("../fonts/DMSans.ttf");

/// The text of the viewport when no run is open.
const NO_RUN: &str = "No run is open. Select a run at the top, or ask the agent for a model.";

/// What the viewer shows.
pub enum Content {
    /// The results of a run, and the supports and loads of its input or why there are none.
    Results(Box<Results>, Result<Box<Preview>, String>),
    /// The model of a solver input with its supports and loads, before the solve.
    Preview(Box<Preview>),
    /// A text in place of the results, for example while they load.
    Text(String),
    /// The results cannot be shown. The text says why.
    Failed(String),
}

/// A slot that a loader fills later, for example after an HTTP fetch in the browser.
pub type ContentSlot = Arc<Mutex<Option<Content>>>;

/// The results of a run with their view state, or a model before the solve.
enum Body {
    Results(Box<Results>, ViewState, Loads),
    Preview(Box<Preview>),
}

/// The supports and loads of the input of a run.
struct Loads {
    preview: Result<Box<Preview>, String>,
    /// The results have the mesh of the input, so the labels move with the deformed shape.
    same_mesh: bool,
}

struct Shown {
    body: Body,
    geometry: Option<Arc<Geometry>>,
    version: u64,
    camera: Camera,
    home: Camera,
}

pub struct ViewerApp {
    render_state: Option<egui_wgpu::RenderState>,
    shown: Option<Shown>,
    message: Option<(bool, String)>,
    /// The page fills the slot when it opens a run, then asks for a repaint.
    slot: ContentSlot,
    /// Show the labels and the arrows of the supports and loads on the model.
    labels: bool,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, slot: ContentSlot) -> Self {
        style(&cc.egui_ctx);
        Self {
            render_state: cc.wgpu_render_state.clone(),
            shown: None,
            message: Some((false, NO_RUN.into())),
            slot,
            labels: true,
        }
    }

    fn set_content(&mut self, content: Content) {
        let body = match content {
            Content::Results(r, preview) => {
                let view = ViewState::new(&r);
                let same_mesh = preview.as_ref().is_ok_and(|p| p.matches(&r.mesh));
                Body::Results(r, view, Loads { preview, same_mesh })
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
            camera: Camera::fit(None),
            home: Camera::fit(None),
        };
        if let Some(rs) = self.render_state.as_ref() {
            let gpu = MeshGpu::new(rs, shown.mesh());
            rs.renderer.write().callback_resources.insert(gpu);
        }
        shown.home = Camera::fit(shown.view_bounds());
        shown.camera = shown.home;
        self.message = None;
        shown.refresh();
        self.shown = Some(shown);
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
            Body::Results(r, _, Loads { preview: Ok(p), .. }) => (results::view_bounds(r)?, p),
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

    fn mesh(&self) -> &RenderMesh {
        match &self.body {
            Body::Results(r, ..) => &r.mesh,
            Body::Preview(p) => &p.mesh,
        }
    }

    /// Make new vertex data after a change of the view state.
    fn refresh(&mut self) {
        self.version += 1;
        let geometry = match &self.body {
            Body::Results(r, v, _) => v.geometry(r, self.version),
            Body::Preview(p) => Geometry {
                version: self.version,
                positions: Vec::new(),
                faces: face_vertices(&p.mesh, &p.mesh.positions, &p.face_values()),
            },
        };
        self.geometry = Some(Arc::new(geometry));
    }

    /// The draw parameters of the body: the contour, the colours and the ghost outline.
    fn frame(&self) -> (bool, Option<[[f32; 3]; BANDS]>, bool) {
        match &self.body {
            Body::Results(r, v, _) => (
                matches!(v.contour(r), Contour::On { .. }),
                None,
                v.deformed && v.ghost,
            ),
            Body::Preview(_) => (true, Some(COLORS), false),
        }
    }

    /// The panel of the body. Returns true when the view state changed.
    fn panel(&mut self, ui: &mut egui::Ui, labels: &mut bool) -> bool {
        match &mut self.body {
            Body::Results(r, v, loads) => {
                let changed = results_panel(ui, r, v);
                loads_panel(ui, loads, labels);
                changed
            }
            Body::Preview(p) => {
                preview_panel(ui, p, labels);
                false
            }
        }
    }

    /// The supports and loads on top of the model, with the positions of the drawn mesh.
    fn overlay(&self, painter: &egui::Painter, rect: egui::Rect, m: &camera::Mat4) {
        let eye = self.camera.eye();
        match &self.body {
            Body::Preview(p) => loads_overlay(painter, rect, m, eye, p, &p.mesh.positions, false),
            Body::Results(_, _, loads) => {
                let Ok(p) = &loads.preview else { return };
                let positions = match (&self.geometry, loads.same_mesh) {
                    (Some(g), true) => &g.positions,
                    _ => &p.mesh.positions,
                };
                loads_overlay(painter, rect, m, eye, p, positions, true);
            }
        }
    }
}

fn kind_text(kind: Kind) -> &'static str {
    match kind {
        Kind::Support => "support",
        Kind::Load => "load",
    }
}

/// The supports and loads of a run under its results: a switch and the label of each set.
fn loads_panel(ui: &mut egui::Ui, loads: &Loads, labels: &mut bool) {
    ui.separator();
    ui.strong("Supports and loads");
    let p = match &loads.preview {
        Ok(p) => p,
        Err(e) => {
            ui.weak(format!("Not available: {e}"));
            return;
        }
    };
    ui.checkbox(labels, "Show them on the model");
    for item in &p.items {
        ui.horizontal(|ui| {
            swatch(ui, COLORS[item.color]);
            ui.label(&item.label);
        });
    }
    if !loads.same_mesh {
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
    ui.checkbox(labels, "Show the labels and the arrows");
    if p.items.is_empty() {
        let error = ui.visuals().error_fg_color;
        ui.colored_label(error, "The input has no support and no load.");
    }
    for item in &p.items {
        ui.separator();
        ui.horizontal(|ui| {
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
) {
    let screen = |x: [f32; 3]| {
        let c = camera::transform(m, x);
        (c[3] > 1e-6).then(|| {
            let (x, y) = (c[0] / c[3], c[1] / c[3]);
            egui::pos2(
                rect.left() + (x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - y) * 0.5 * rect.height(),
            )
        })
    };
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
        let [r, g, b] = COLORS[item.color];
        let text = if 0.3 * r + 0.6 * g + 0.1 * b > 0.5 {
            egui::Color32::BLACK
        } else {
            egui::Color32::WHITE
        };
        let font = egui::FontId::proportional(14.0);
        let galley = painter.layout_no_wrap(item.label.clone(), font, text);
        let place =
            egui::Align2::LEFT_BOTTOM.anchor_size(at + egui::vec2(6.0, -6.0), galley.size());
        // Keep the label in the view.
        let inner = rect.shrink(4.0);
        let dx = (inner.right() - place.right())
            .min(0.0)
            .max(inner.left() - place.left());
        let dy = (inner.bottom() - place.bottom())
            .min(0.0)
            .max(inner.top() - place.top());
        let place = place.translate(egui::vec2(dx, dy));
        painter.rect_filled(place.expand(3.0), 3.0, color);
        painter.galley(place.min, galley, text);
    }
}

fn rgb(c: [f32; 3]) -> egui::Color32 {
    let c = c.map(|x| (x * 255.0).round() as u8);
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

fn swatch(ui: &mut egui::Ui, color: [f32; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, rgb(color));
}

/// The results panel. Returns true when the view state changed.
fn results_panel(ui: &mut egui::Ui, r: &Results, v: &mut ViewState) -> bool {
    let before = v.clone();
    ui.heading("Results");
    ui.label(format!("Units: {}", r.units));
    ui.separator();

    ui.strong("Step");
    let n = r.steps.len();
    // Before the controls, so that they show the step of the shape.
    if let Some(wait) = v.play(ui.input(|i| i.time), n) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(v.step > 0, egui::Button::new("<"))
            .on_hover_text("previous step")
            .clicked()
        {
            v.step -= 1;
        }
        egui::ComboBox::from_id_salt("step")
            .selected_text(view::step_label(v.step, &r.steps[v.step]))
            .show_ui(ui, |ui| {
                for (i, s) in r.steps.iter().enumerate() {
                    ui.selectable_value(&mut v.step, i, view::step_label(i, s));
                }
            });
        if ui
            .add_enabled(v.step + 1 < n, egui::Button::new(">"))
            .on_hover_text("next step")
            .clicked()
        {
            v.step += 1;
        }
    });
    if n > 1 {
        ui.horizontal(|ui| {
            let text = if v.playing.is_some() { "Pause" } else { "Play" };
            if ui
                .button(text)
                .on_hover_text("show each step in turn")
                .clicked()
            {
                v.playing = match v.playing {
                    Some(_) => None,
                    None => Some(ui.input(|i| i.time)),
                };
            }
            ui.add(egui::Slider::new(&mut v.step, 0..=n - 1).show_value(false));
        });
    }
    ui.label(format!("{n} step{}", if n == 1 { "" } else { "s" }));
    ui.separator();

    ui.strong("Deformed shape");
    ui.checkbox(&mut v.deformed, "Show the deformed shape");
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
            if ui.button("Auto").clicked() {
                v.scale = results::auto_scale(r);
            }
            if ui.button("1x").on_hover_text("true scale").clicked() {
                v.scale = 1.0;
            }
        });
        ui.checkbox(&mut v.ghost, "Undeformed outline");
    });
    let step = v.current(r);
    let unit = results::unit(&r.units, results::Quantity::Displacement);
    ui.label(format!(
        "Largest displacement: {} {unit}",
        view::format_value(step.max_displacement())
    ));
    ui.separator();

    ui.strong("Contour");
    let names = view::field_names(r);
    egui::ComboBox::from_id_salt("field")
        .selected_text(v.field.clone().unwrap_or_else(|| "None".into()))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut v.field, None, "None");
            for name in &names {
                ui.selectable_value(&mut v.field, Some(name.clone()), name);
            }
        });
    match v.contour(r) {
        Contour::Off => {}
        Contour::Missing(name) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Step {} has no field {name}.", v.step + 1),
            );
        }
        Contour::On { field, lo, hi } => legend(ui, &r.units, field, lo, hi),
    }
    *v != before
}

/// The colour bands with their limits, the highest band at the top.
fn legend(ui: &mut egui::Ui, units: &str, field: &results::ScalarField, lo: f64, hi: f64) {
    let unit = results::unit(units, field.quantity);
    let unit_text = if unit.is_empty() {
        String::new()
    } else {
        format!(" ({unit})")
    };
    let at = match field.location {
        Location::Point => "at the nodes",
        Location::Cell => "per element",
    };
    ui.label(format!("{}{unit_text}, {at}", field.name));
    let swatch = |ui: &mut egui::Ui, band: usize, text: String| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 14.0), egui::Sense::hover());
            let c = BAND_COLORS[band].map(|x| (x * 255.0).round() as u8);
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_rgb(c[0], c[1], c[2]));
            ui.monospace(text);
        });
    };
    if hi <= lo {
        swatch(
            ui,
            BANDS / 2,
            format!("constant {}", view::format_value(lo)),
        );
        return;
    }
    let limits = results::band_limits(lo, hi);
    let texts = view::format_limits(&limits);
    for i in (0..BANDS).rev() {
        let text = format!("{} to {}", texts[i], texts[i + 1]);
        swatch(ui, i, text);
    }
    ui.label(format!(
        "Min {}, max {}",
        view::format_value(lo),
        view::format_value(hi)
    ));
}

/// The look of the page (`crates/fh/src/daemon/page.html`): the DM Sans font, and the colours
/// `--bg`, `--raised`, `--line` and `--text` of the page in the dark and the light theme.
fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let sans = egui::FontData::from_static(DM_SANS);
    fonts.font_data.insert("DM Sans".into(), Arc::new(sans));
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "DM Sans".into());
    }
    ctx.set_fonts(fonts);
    let hex = |c: u32| egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
    for (theme, bg, raised, line, text) in [
        (egui::Theme::Dark, 0x1c1b18, 0x2e2c27, 0x3a3730, 0xece8df),
        (egui::Theme::Light, 0xf4f1ea, 0xe6e0d3, 0xd8d2c4, 0x1c1b18),
    ] {
        let [bg, raised, line, text] = [bg, raised, line, text].map(hex);
        ctx.style_mut_of(theme, |s| {
            use egui::{FontId, TextStyle};
            s.text_styles = [
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Body, FontId::proportional(14.0)),
                (TextStyle::Button, FontId::proportional(14.0)),
                (TextStyle::Heading, FontId::proportional(18.0)),
                (TextStyle::Monospace, FontId::monospace(12.5)),
            ]
            .into();
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.spacing.interact_size.y = 26.0;
            let v = &mut s.visuals;
            v.panel_fill = bg;
            v.window_fill = bg;
            v.widgets.noninteractive.bg_stroke.color = line;
            v.widgets.inactive.bg_fill = raised;
            v.widgets.inactive.weak_bg_fill = raised;
            v.override_text_color = Some(text);
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = egui::CornerRadius::same(8);
            }
        });
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let next = self.slot.lock().ok().and_then(|mut s| s.take());
        if let Some(content) = next {
            self.set_content(content);
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.weak(self.status_text());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak("drag: orbit | right or shift drag: pan | scroll: zoom | double click: reset");
                });
            });
        });
        if let Some(shown) = self.shown.as_mut() {
            egui::Panel::right("results")
                .resizable(false)
                .min_size(260.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if shown.panel(ui, &mut self.labels) {
                            shown.refresh();
                        }
                    });
                });
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
            let delta = response.drag_delta();
            let pan = response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || (response.dragged_by(egui::PointerButton::Primary)
                    && ui.input(|i| i.modifiers.shift));
            if pan {
                shown.camera.pan(delta.x, delta.y, rect.height());
            } else if response.dragged_by(egui::PointerButton::Primary) {
                shown.camera.orbit(delta.x, delta.y);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                shown.camera.zoom(scroll);
            }
            if response.double_clicked() {
                shown.camera = shown.home;
            }
            let (contour, palette, ghost) = shown.frame();
            let view_proj = shown
                .camera
                .view_proj(rect.width() / rect.height().max(1.0));
            let params = FrameParams {
                view_proj,
                eye: shown.camera.eye(),
                dark: ui.visuals().dark_mode,
                contour,
                ghost,
                palette,
            };
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                MeshCallback {
                    params,
                    geometry: shown.geometry.clone(),
                },
            ));
            if self.labels {
                shown.overlay(&ui.painter_at(rect), rect, &view_proj);
            }
        });
    }
}
