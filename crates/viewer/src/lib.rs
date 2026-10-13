//! eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree.
//!
//! Built so far: the mesh-only viewer behind `fh view`. `mesh` and `camera` are plain
//! functions with no GPU types. `gpu` and `app` are the thin window layer.

mod app;
pub mod camera;
mod gpu;
pub mod mesh;

use std::path::Path;

/// Read an `.inp` file and open the viewer on it. Returns when the window closes.
pub fn view_file(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let model = fh_inp::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let render = mesh::build(&model);
    if render.triangles.is_empty() {
        return Err(format!(
            "{}: no C3D4, C3D8 or C3D10 elements to draw ({} nodes, {} elements)",
            path.display(),
            render.node_count,
            render.element_count
        ));
    }
    let title = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    app::run(title, render)
}
