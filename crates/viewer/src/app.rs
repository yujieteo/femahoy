//! The eframe app: a viewport, a results panel, a status bar and the mouse controls, on the
//! page of `fh serve`. The chat panel is on the left of the viewport.

use std::sync::{Arc, Mutex};

use eframe::egui;
use eframe::egui_wgpu;

use crate::camera::Camera;
use crate::chat::Chat;
use crate::gpu::{FrameParams, Geometry, MeshCallback, MeshGpu};
use crate::results::{self, Location, Results, BANDS, BAND_COLORS};
use crate::view::{self, Contour, ViewState};

/// What the viewer shows.
pub enum Content {
    /// The results of a run.
    Results(Box<Results>),
    /// The runs with results that the page can open: a label and a page address each.
    Runs(Vec<(String, String)>),
    /// The results are still loading. The text says what.
    Loading(String),
    /// The results cannot be shown. The text says why.
    Failed(String),
}

/// A slot that a loader fills later, for example after an HTTP fetch in the browser.
pub type ContentSlot = Arc<Mutex<Option<Content>>>;

struct Shown {
    results: Box<Results>,
    view: ViewState,
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
        let r = match content {
            Content::Results(r) => r,
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
        if let Some(rs) = self.render_state.as_ref() {
            let gpu = MeshGpu::new(rs, &r.mesh);
            rs.renderer.write().callback_resources.insert(gpu);
        }
        let home = Camera::fit(r.mesh.bounds);
        self.message = None;
        let mut shown = Shown {
            view: ViewState::new(&r),
            results: r,
            geometry: None,
            version: 0,
            camera: home,
            home,
        };
        shown.refresh();
        self.shown = Some(shown);
    }

    fn status_text(&self) -> String {
        let Some(s) = &self.shown else {
            return self.title.clone();
        };
        let m = &s.results.mesh;
        let mut text = format!(
            "{}: {} nodes, {} elements, {} surface triangles",
            self.title,
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
    /// Make new vertex data after a change of the view state.
    fn refresh(&mut self) {
        self.version += 1;
        self.geometry = Some(Arc::new(self.view.geometry(&self.results, self.version)));
    }

    fn contour_on(&self) -> bool {
        matches!(self.view.contour(&self.results), Contour::On { .. })
    }

    /// The results panel. Returns true when the view state changed.
    fn panel(&mut self, ui: &mut egui::Ui) -> bool {
        let (r, v) = (&self.results, &mut self.view);
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
            let deformed = shown.view.deformed && shown.view.ghost;
            let params = FrameParams {
                view_proj: shown
                    .camera
                    .view_proj(rect.width() / rect.height().max(1.0)),
                eye: shown.camera.eye(),
                dark: ui.visuals().dark_mode,
                contour: shown.contour_on(),
                ghost: deformed,
            };
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                MeshCallback {
                    params,
                    geometry: shown.geometry.clone(),
                },
            ));
        });
    }
}
