//! The eframe app: a viewport, a status bar and the mouse controls.

use eframe::egui;
use eframe::egui_wgpu;

use crate::camera::Camera;
use crate::gpu::{self, FrameParams, MeshCallback, MeshGpu};
use crate::mesh::RenderMesh;

pub struct ViewerApp {
    title: String,
    mesh: RenderMesh,
    camera: Camera,
    home: Camera,
}

impl ViewerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, title: String, mesh: RenderMesh) -> Self {
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let gpu = MeshGpu::new(rs, &mesh);
            rs.renderer.write().callback_resources.insert(gpu);
        }
        let home = Camera::fit(mesh.bounds);
        Self {
            title,
            mesh,
            camera: home,
            home,
        }
    }

    fn status_text(&self) -> String {
        let m = &self.mesh;
        let mut text = format!(
            "{}: {} nodes, {} elements, {} surface triangles",
            self.title,
            m.node_count,
            m.element_count,
            m.triangles.len()
        );
        if m.skipped_elements > 0 {
            text.push_str(&format!(
                ", {} elements of an unsupported type not drawn",
                m.skipped_elements
            ));
        }
        text
    }
}

impl eframe::App for ViewerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.status_text());
                ui.separator();
                ui.weak(
                    "drag: orbit | right or shift drag: pan | scroll: zoom | double click: reset",
                );
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let size = ui.available_size();
            let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
            let delta = response.drag_delta();
            let pan = response.dragged_by(egui::PointerButton::Secondary)
                || response.dragged_by(egui::PointerButton::Middle)
                || (response.dragged_by(egui::PointerButton::Primary)
                    && ui.input(|i| i.modifiers.shift));
            if pan {
                self.camera.pan(delta.x, delta.y, rect.height());
            } else if response.dragged_by(egui::PointerButton::Primary) {
                self.camera.orbit(delta.x, delta.y);
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                self.camera.zoom(scroll);
            }
            if response.double_clicked() {
                self.camera = self.home;
            }
            let params = FrameParams {
                view_proj: self.camera.view_proj(rect.width() / rect.height().max(1.0)),
                eye: self.camera.eye(),
                dark: ui.visuals().dark_mode,
            };
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                MeshCallback(params),
            ));
        });
    }
}

/// Open a window and run until it closes.
pub fn run(title: String, mesh: RenderMesh) -> Result<(), String> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: gpu::DEPTH_BITS,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 760.0])
            .with_title(format!("fh view {title}")),
        ..Default::default()
    };
    eframe::run_native(
        "fh view",
        options,
        Box::new(move |cc| Ok(Box::new(ViewerApp::new(cc, title, mesh)))),
    )
    .map_err(|e| e.to_string())
}
