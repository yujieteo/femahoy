//! The browser entry point. The page of `fh serve` (`crates/fh/src/daemon/page.html`) calls
//! `start` with its canvas and the token, and `Viewer::open` for each run that it shows.
//! The viewer fetches the result files from the daemon with `Authorization: Bearer
//! <token>`. A report (`crates/fh/src/daemon/report.html`) also gives the view of a run that
//! the agent writes to `Viewer::open`, the step of a slider to `Viewer::step` and its switch
//! "Show all controls" to `Viewer::all`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use eframe::egui;
use wasm_bindgen::{prelude::*, JsCast};

use crate::app::{Content, Request, RequestSlot, ViewerApp};
use crate::{preview, results, view, RESULTS_FILE};

/// GET a daemon path. An HTTP error gives the `error` and `message` fields of the answer.
async fn get(token: &str, path: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let request = web_sys::Request::new_with_str(path).map_err(|e| format!("{path}: {e:?}"))?;
    request
        .headers()
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(|e| format!("{path}: {e:?}"))?;
    let answer = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{path}: the daemon does not answer ({e:?})"))?;
    let response: web_sys::Response = answer.dyn_into().map_err(|_| "not a response")?;
    let buffer = wasm_bindgen_futures::JsFuture::from(
        response
            .array_buffer()
            .map_err(|e| format!("{path}: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("{path}: {e:?}"))?;
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    if !response.ok() {
        let text = String::from_utf8_lossy(&bytes);
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("")
                .to_owned()
        };
        return Err(format!(
            "{path}: HTTP {}: {}",
            response.status(),
            field("message")
        ));
    }
    Ok(bytes)
}

fn text(bytes: Vec<u8>, path: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8 text"))
}

/// Fetch the result files of a run and read them, with the supports and loads of its input and
/// the view `view` (the text of [`view::View::parse`]).
async fn load_run(token: &str, run: &str, view: &str) -> Result<Content, String> {
    let view = view::View::parse(view).map_err(|e| format!("Run {run}: the view: {e}"))?;
    let base = format!("/api/v1/runs/{run}/files");
    let pvd_path = format!("{base}/{RESULTS_FILE}");
    let pvd = text(get(token, &pvd_path).await?, &pvd_path)?;
    let entries = fh_core::vtu::read_pvd(&pvd).map_err(|e| format!("{RESULTS_FILE}: {e}"))?;
    let mut files = HashMap::new();
    for e in &entries {
        if !results::is_plain_file_name(&e.file) {
            continue;
        }
        let path = format!("{base}/{}", e.file);
        files.insert(e.file.clone(), get(token, &path).await);
    }
    let r = results::load(RESULTS_FILE, &pvd, |f| {
        files.remove(f).unwrap_or_else(|| Err("not fetched".into()))
    })
    .map_err(|e| format!("Run {run}: {e}"))?;
    let loads = model(token, &format!("/api/v1/runs/{run}/input"), run).await;
    Ok(Content::Results(Box::new(r), loads, view))
}

/// Fetch an input file from the daemon path `path` and read its supports and loads.
async fn model(token: &str, path: &str, name: &str) -> Result<Box<preview::Preview>, String> {
    let input = text(get(token, path).await?, path)?;
    let mut p = preview::build(&fh_core::inp::parse(&input), &fh_core::inp::units(&input));
    p.title = name.to_owned();
    Ok(Box::new(p))
}

/// The viewer on the canvas of the page.
#[wasm_bindgen]
pub struct Viewer {
    token: String,
    slot: RequestSlot,
    ctx: egui::Context,
    /// The number of the last `open`. A load that a later `open` replaced shows nothing.
    opened: Rc<Cell<u64>>,
}

#[wasm_bindgen]
impl Viewer {
    /// Show the results of a run in place of the current content, with the view of a report.
    /// With no view, the panel shows each control that applies and the important fields of the run.
    pub fn open(&self, run: String, view: Option<String>) {
        self.ask(|r| r.mesh = Some(false));
        let token = self.token.clone();
        self.load("Loading the results...", async move {
            load_run(&token, &run, view.as_deref().unwrap_or("")).await
        });
    }

    /// Open only the undeformed mesh, with no contour or independent scale.
    pub fn mesh(&self, run: String) {
        self.ask(|r| r.mesh = Some(true));
        let token = self.token.clone();
        self.load("Loading the mesh...", async move {
            load_run(&token, &run, view::MESH_VIEW).await
        });
    }

    /// Link the absolute camera pose and zoom of this pair. Loads remain independent.
    pub fn link(&self, other: &Viewer, on: bool) {
        let shared = on.then(|| {
            Arc::new(Mutex::new(crate::app::LinkedCamera {
                camera: None,
                contexts: [self.ctx.clone(), other.ctx.clone()],
            }))
        });
        self.ask(|r| r.camera_link = Some(shared.clone()));
        other.ask(|r| r.camera_link = Some(shared));
    }

    /// Accessible load state and mesh counts for the page beside the canvas.
    pub fn status(&self) -> String {
        self.slot
            .lock()
            .map(|r| r.status.clone())
            .unwrap_or_default()
    }

    /// Show the model of a preview with its supports and loads, before the solve.
    pub fn show(&self, preview: String, name: String) {
        let token = self.token.clone();
        self.load("Loading the model...", async move {
            let path = format!("/api/v1/previews/{preview}");
            model(&token, &path, &name).await.map(Content::Preview)
        });
    }

    /// Show the step `step` (from 1) of the open run, or of the run that loads.
    pub fn step(&self, step: usize) {
        self.ask(|r| r.step = Some(step.saturating_sub(1)));
    }

    /// Show all the controls of a run, also the controls that its view does not show.
    pub fn all(&self, all: bool) {
        self.ask(|r| r.all = Some(all));
    }
}

impl Viewer {
    fn ask(&self, change: impl FnOnce(&mut Request)) {
        if let Ok(mut r) = self.slot.lock() {
            change(&mut r);
        }
        self.ctx.request_repaint();
    }

    fn load(
        &self,
        loading: &str,
        work: impl std::future::Future<Output = Result<Content, String>> + 'static,
    ) {
        let n = self.opened.get() + 1;
        self.opened.set(n);
        let fill = |slot: &RequestSlot, ctx: &egui::Context, content| {
            if let Ok(mut s) = slot.lock() {
                s.content = Some(content);
            }
            ctx.request_repaint();
        };
        // A step that the page asked for before this load is for the run before.
        self.ask(|r| r.step = None);
        fill(&self.slot, &self.ctx, Content::Text(loading.into()));
        let (slot, ctx, opened) = (self.slot.clone(), self.ctx.clone(), self.opened.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let content = work.await;
            if opened.get() == n {
                fill(&slot, &ctx, content.unwrap_or_else(Content::Failed));
            }
        });
    }
}

/// Start the viewer on a canvas of the page, with the token of the daemon.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement, token: String) -> Result<Viewer, JsValue> {
    let slot: RequestSlot = Arc::new(Mutex::new(Request::default()));
    let control_canvas = canvas.clone();
    let canvas_id = canvas.id();
    let ctx: Rc<RefCell<Option<egui::Context>>> = Rc::default();
    let options = eframe::WebOptions {
        depth_buffer: crate::gpu::DEPTH_BITS,
        ..Default::default()
    };
    let (app_slot, app_ctx) = (slot.clone(), ctx.clone());
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(move |cc| {
                *app_ctx.borrow_mut() = Some(cc.egui_ctx.clone());
                Ok(Box::new(ViewerApp::new(cc, app_slot, canvas_id)))
            }),
        )
        .await?;
    let ctx = ctx.take().ok_or("the viewer did not start")?;
    crate::controls::install(&control_canvas, slot.clone(), ctx.clone())?;
    Ok(Viewer {
        token,
        slot,
        ctx,
        opened: Rc::default(),
    })
}
