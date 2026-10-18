//! eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree.
//!
//! Built so far: the viewer behind `fh view` and the page of `fh serve`. `mesh`, `camera`,
//! `results` and `view` are plain functions with no GPU types. `gpu` and `app` are the thin
//! window layer. `web` is the browser entry point.

mod app;
pub mod camera;
mod gpu;
pub mod mesh;
pub mod results;
pub mod view;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::Content;

/// The results file of a run folder.
pub const RESULTS_FILE: &str = "results.pvd";

/// Read the results of a run from a run folder or its `results.pvd`.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_run(path: &std::path::Path) -> Result<results::Results, String> {
    let pvd_path = if path.extension().is_some_and(|e| e == "pvd") {
        path.to_path_buf()
    } else {
        path.join(RESULTS_FILE)
    };
    let dir = pvd_path
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let pvd =
        std::fs::read_to_string(&pvd_path).map_err(|e| format!("{}: {e}", pvd_path.display()))?;
    results::load(&pvd_path.display().to_string(), &pvd, |file| {
        std::fs::read(dir.join(file)).map_err(|e| e.to_string())
    })
    .map_err(|e| {
        if e.file == pvd_path.display().to_string() {
            e.to_string()
        } else {
            format!("{}: {e}", dir.display())
        }
    })
}

/// Open the viewer on an `.inp` model, on a run folder, or on a `results.pvd`. Returns when
/// the window closes.
#[cfg(not(target_arch = "wasm32"))]
pub fn view_file(path: &std::path::Path) -> Result<(), String> {
    let title = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let is_results = path.is_dir() || path.extension().is_some_and(|e| e == "pvd");
    if is_results {
        let r = load_run(path)?;
        return app::run(title, Content::Results(Box::new(r)));
    }
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
    app::run(title, Content::Mesh(render))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn load_run_takes_the_folder_or_the_pvd() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/results/cantilever-2step");
        let a = load_run(&dir).unwrap();
        let b = load_run(&dir.join(RESULTS_FILE)).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.steps.len(), 2);
    }

    #[test]
    fn load_run_names_a_missing_folder() {
        let e = load_run(std::path::Path::new("/no/such/run")).unwrap_err();
        assert!(e.starts_with("/no/such/run/results.pvd: "), "{e}");
    }
}
