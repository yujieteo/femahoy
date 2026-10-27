//! eframe app, wgpu mesh and contour renderer.
//!
//! Built so far: the model view of the page of `fh serve`. The page itself, with the chat,
//! is HTML (`crates/fh/src/daemon/page.html`). `mesh`, `camera`, `results` and `view` are
//! plain functions with no GPU types. `solve` solves a `grid.toml` case in a worker of the page. `ui` has the design tokens of each view of femahoy, and
//! `widgets` has the components of that UI layer in egui. `gpu` and `app` are the thin window
//! layer. `web` is the browser entry point. Only the browser uses `gpu`, `app`, `widgets` and
//! `web`, so only the wasm build compiles them.

#[cfg(target_arch = "wasm32")]
mod app;
pub mod camera;
#[cfg(target_arch = "wasm32")]
mod controls;
#[cfg(target_arch = "wasm32")]
mod gpu;
pub mod mesh;
pub mod preview;
pub mod probe;
pub mod results;
pub mod solve;
pub mod ui;
pub mod view;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
mod widgets;

#[cfg(target_arch = "wasm32")]
pub use app::{Content, Request, RequestSlot, ViewerApp};

/// The results file of a run folder.
pub const RESULTS_FILE: &str = "results.pvd";

#[cfg(test)]
fn load_run(dir: &std::path::Path) -> Result<results::Results, String> {
    let pvd_path = dir.join(RESULTS_FILE);
    let pvd =
        std::fs::read_to_string(&pvd_path).map_err(|e| format!("{}: {e}", pvd_path.display()))?;
    results::load(&pvd_path.display().to_string(), &pvd, |file| {
        std::fs::read(dir.join(file)).map_err(|e| e.to_string())
    })
    .map_err(|e| e.to_string())
}
