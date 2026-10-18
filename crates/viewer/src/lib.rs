//! eframe app, wgpu mesh and contour renderer, terminal widget, chat panel, teammate tree.
//!
//! Built so far: the page of `fh serve`, with the chat panel and the terminal sidebar on the
//! page. `mesh`, `camera`, `results` and `view` are plain functions with no GPU types.
//! `terminal` decodes the screens of the daemon and encodes the keys, `sidebar` holds the tabs
//! and the panes, and `chat` shows the actions of the leader. `gpu` and `app` are the thin
//! window layer. `web` is the browser entry point.

mod app;
pub mod camera;
pub mod chat;
mod gpu;
pub mod mesh;
pub mod results;
pub mod sidebar;
pub mod terminal;
pub mod view;
#[cfg(target_arch = "wasm32")]
mod web;

pub use app::{Content, ContentSlot, ViewerApp};

/// The results file of a run folder.
pub const RESULTS_FILE: &str = "results.pvd";
