//! eframe app, wgpu mesh and contour renderer, chat panel, teammate tree.
//!
//! Built so far: the page of `fh serve`, with the chat panel on the page. `mesh`, `camera`,
//! `results` and `view` are plain functions with no GPU types. `chat` shows the actions of
//! the leader. `gpu` and `app` are the thin window layer. `web` is the browser entry point.

mod app;
pub mod camera;
pub mod chat;
mod gpu;
pub mod mesh;
pub mod results;
pub mod view;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{Content, ContentSlot, ViewerApp};

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
