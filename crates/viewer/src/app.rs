//! The eframe app: a viewport, a results panel, a status bar and the mouse controls, on the
//! page of `fh serve`. The chat panel is on the left of the viewport. Before a solve, the
//! viewport shows the model with its supports and loads: the faces of each set in a colour,
//! an arrow for each load and the set names.

use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu;

use crate::camera::{self, Camera};
use crate::chat::Chat;
use crate::gpu::{face_vertices, FrameParams, Geometry, MeshCallback, MeshGpu};
use crate::mesh::RenderMesh;
use crate::preview::{Kind, Preview, COLORS};
use crate::results::{self, Location, Results, BANDS, BAND_COLORS};
use crate::view::{self, Contour, ViewState};

/// What the viewer shows.
pub enum Content {
    /// The results of a run.
    Results(Box<Results>),
    /// The model of a solver input with its supports and loads, before the solve.
    Preview(Box<Preview>),
    /// The runs with results that the page can open: a label and a page address each.
    Runs(Vec<(String, String)>),
    /// The results are still loading. The text says what.
    Loading(String),
    /// The results cannot be shown. The text says why.
    Failed(String),
}

/// A slot that a loader fills later, for example after an HTTP fetch in the browser.
pub type ContentSlot = Arc<Mutex<Option<Content>>>;

/// The results of a run with their view state, or a model before the solve.
enum Body {
    Results(Box<Results>, ViewState),
    Preview(Box<Preview>),
}

struct Shown {
    body: Body,
    geometry: Option<Arc<Geometry>>,
    version: u64,
    camera: Camera,
    home: Camera,
}

pub struct ViewerApp {
    title: String,
    render_state: Option<egui_wgpu::RenderState>,
    shown: Option<Shown>,
    message: Option<(bool, String)>,
    runs: Option<Vec<(String, String)>>,
    slot: Option<ContentSlot>,
    /// The chat with the leader: only on the page of `fh serve`.
    chat: Option<Chat>,
    chat_open: bool,
}

impl ViewerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        title: String,
        content: Content,
        slot: Option<ContentSlot>,
    ) -> Self {
        let mut app = Self {
            title,
            render_state: cc.wgpu_render_state.clone(),
            shown: None,
            message: None,
            runs: None,
            slot,
            chat: None,
            chat_open: true,
        };
        app.set_content(content);
        app
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_chat(&mut self, chat: Chat) {
        self.chat = Some(chat);
    }

    fn set_content(&mut self, content: Content) {
        if !matches!(content, Content::Runs(_)) {
            self.runs = None;
        }
        let body = match content {
            Content::Results(r) => {
                let view = ViewState::new(&r);
                Body::Results(r, view)
            }
            Content::Preview(p) => Body::Preview(p),
            Content::Runs(links) => {
                self.message = None;
                self.runs = Some(links);
                return;
            }
            Content::Loading(text) => {
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
        shown.home = Camera::fit(shown.mesh().bounds);
        shown.camera = shown.home;
        self.message = None;
        shown.refresh();
        self.shown = Some(shown);
    }

    fn status_text(&self) -> String {
        let Some(s) = &self.shown else {
            return self.title.clone();
        };
        let m = s.mesh();
        let title = match &s.body {
            Body::Preview(p) => format!("{} before the solve", p.title),
            Body::Results(..) => self.title.clone(),
        };
        let mut text = format!(
            "{title}: {} nodes, {} elements, {} surface triangles",
            m.node_count,
            m.element_count,
            m.triangles.len()
        );
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
    fn mesh(&self) -> &RenderMesh {
        match &self.body {
            Body::Results(r, _) => &r.mesh,
            Body::Preview(p) => &p.mesh,
        }
    }

    /// Make new vertex data after a change of the view state.
    fn refresh(&mut self) {
        self.version += 1;
        let geometry = match &self.body {
            Body::Results(r, v) => v.geometry(r, self.version),
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
            Body::Results(r, v) => (
                matches!(v.contour(r), Contour::On { .. }),
                None,
                v.deformed && v.ghost,
            ),
            Body::Preview(_) => (true, Some(COLORS), false),
        }
    }

    /// The panel of the body. Returns true when the view state changed.
    fn panel(&mut self, ui: &mut egui::Ui) -> bool {
        match &mut self.body {
            Body::Results(r, v) => results_panel(ui, r, v),
            Body::Preview(p) => {
                preview_panel(ui, p);
                false
            }
        }
    }
}

/// The supports and loads with their colours and their texts.
fn preview_panel(ui: &mut egui::Ui, p: &Preview) {
    ui.heading("Supports and loads");
    ui.label(format!(
        "{}: the input of the solve, before the solve.",
        p.title
    ));
    ui.label(format!("Units: {}", p.units));
    if p.items.is_empty() {
        let error = ui.visuals().error_fg_color;
        ui.colored_label(error, "The input has no support and no load.");
    }
    for item in &p.items {
        ui.separator();
        ui.horizontal(|ui| {
            swatch(ui, COLORS[item.color]);
            ui.strong(&item.name);
            ui.weak(match item.kind {
                Kind::Support => "support",
                Kind::Load => "load",
            });
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

/// The names of the sets and an arrow for each load, on top of the model.
fn preview_overlay(painter: &egui::Painter, rect: egui::Rect, m: &camera::Mat4, p: &Preview) {
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
    for item in &p.items {
        let color = rgb(COLORS[item.color]);
        let mut at = screen(item.center.map(|x| x as f32));
        if let Some((tail, head)) = p.arrow(item) {
            if let (Some(a), Some(b)) = (screen(tail), screen(head)) {
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
        let galley = painter.layout_no_wrap(item.name.clone(), font, text);
        let place =
            egui::Align2::LEFT_BOTTOM.anchor_size(at + egui::vec2(6.0, -6.0), galley.size());
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

/// The page with no run selected: a link to each run that has results.
fn run_list(ui: &mut egui::Ui, links: &[(String, String)]) {
    ui.heading("Runs with results");
    if links.is_empty() {
        ui.label(
            "No run has succeeded yet. Submit a run with the run contract \
             (POST /api/v1/projects/<p>/runs), then reload this page.",
        );
        return;
    }
    for (label, url) in links {
        ui.add(egui::Hyperlink::from_label_and_url(label, url).open_in_new_tab(false));
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(slot) = &self.slot {
            let next = slot.lock().ok().and_then(|mut s| s.take());
            if let Some(content) = next {
                self.set_content(content);
            }
            if self.shown.is_none() && matches!(self.message, Some((false, _))) {
                // Still loading: look at the slot again soon.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        if let Some(c) = &mut self.chat {
            c.poll();
        }
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.chat.is_some() {
                    ui.toggle_value(&mut self.chat_open, "Chat")
                        .on_hover_text("show or hide the chat with the leader");
                    ui.separator();
                }
                ui.label(self.status_text());
                ui.separator();
                ui.weak(
                    "drag: orbit | right or shift drag: pan | scroll: zoom | double click: reset",
                );
            });
        });
        if let (Some(c), true) = (&mut self.chat, self.chat_open) {
            let width = (ui.available_width() * 0.3).clamp(300.0, 560.0);
            egui::Panel::left("chat")
                .resizable(true)
                .default_size(width)
                .min_size(260.0)
                .max_size(width.max(560.0))
                .show(ui, |ui| c.ui(ui));
        }
        if let Some(shown) = self.shown.as_mut() {
            egui::Panel::right("results")
                .resizable(false)
                .min_size(250.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if shown.panel(ui) {
                            shown.refresh();
                        }
                    });
                });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(links) = &self.runs {
                run_list(ui, links);
                return;
            }
            if let Some((error, text)) = &self.message {
                ui.centered_and_justified(|ui| {
                    if *error {
                        ui.colored_label(ui.visuals().error_fg_color, text);
                    } else {
                        ui.label(text);
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
            if let Body::Preview(p) = &shown.body {
                preview_overlay(&ui.painter_at(rect), rect, &view_proj, p);
            }
        });
    }
}
