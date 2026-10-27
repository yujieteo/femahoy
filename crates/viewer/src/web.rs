//! The browser entry point. The page of `fh serve` (`crates/fh/src/daemon/page.html`) calls
//! `start` with its canvas and the token, and `Viewer::open` for each run that it shows.
//! The viewer fetches the result files from the daemon with `Authorization: Bearer
//! <token>`. A report (`crates/fh/src/daemon/report.html`) also gives the view of a run that
//! the agent writes to `Viewer::open`, the step of a slider to `Viewer::step` and its switch
//! "Show all controls" to `Viewer::all`.

use std::cell::{Cell, RefCell};
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

/// True while the load that asked is still the last load of its viewer.
type Live = Rc<dyn Fn() -> bool>;

/// The text of a load that a later load replaced. The viewer never shows it.
const REPLACED: &str = "replaced by a later load";

/// Give the browser a frame: a timer of 0 ms ends this task, so the page can draw and take input.
async fn frame() {
    let wait = js_sys::Promise::new(&mut |done, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&done, 0);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(wait).await;
}

/// Fetch the result files of a run and read them, with the supports and loads of its input and
/// the view `view` (the text of [`view::View::parse`]). Each step is fetched and read in its
/// own task, so that a step of a large run does not stop the page for all the steps. When
/// `live` becomes false, the load stops before its next fetch or read.
async fn load_run(token: &str, run: &str, view: &str, live: Live) -> Result<Content, String> {
    let view = view::View::parse(view).map_err(|e| format!("Run {run}: the view: {e}"))?;
    let base = format!("/api/v1/runs/{run}/files");
    let pvd_path = format!("{base}/{RESULTS_FILE}");
    let pvd = text(get(token, &pvd_path).await?, &pvd_path)?;
    let mut loader =
        results::Loader::new(RESULTS_FILE, &pvd).map_err(|e| format!("Run {run}: {e}"))?;
    while let Some(file) = loader.next_file() {
        let path = format!("{base}/{file}");
        let bytes = get(token, &path).await;
        frame().await;
        if !live() {
            return Err(REPLACED.into());
        }
        loader.add(bytes).map_err(|e| format!("Run {run}: {e}"))?;
        frame().await;
        if !live() {
            return Err(REPLACED.into());
        }
    }
    let loads = model(token, &format!("/api/v1/runs/{run}/input"), run).await;
    Ok(Content::Results(Box::new(loader.finish()), loads, view))
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
        self.load("Loading the results...", move |live| async move {
            load_run(&token, &run, view.as_deref().unwrap_or(""), live).await
        });
    }

    /// Open only the undeformed mesh, with no contour or independent scale.
    pub fn mesh(&self, run: String) {
        self.ask(|r| r.mesh = Some(true));
        let token = self.token.clone();
        self.load("Loading the mesh...", move |live| async move {
            load_run(&token, &run, view::MESH_VIEW, live).await
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
        self.load("Loading the model...", move |_| async move {
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

    fn load<W: std::future::Future<Output = Result<Content, String>> + 'static>(
        &self,
        loading: &str,
        work: impl FnOnce(Live) -> W + 'static,
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
            // Start the fetch only after a frame took the text and freed the shown model.
            // Reason: on localhost the fetch and the parse end before the next frame, and the
            // page then holds two A320 models.
            while opened.get() == n && slot.lock().is_ok_and(|s| s.content.is_some()) {
                let wait = js_sys::Promise::new(&mut |done, _| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&done, 16);
                    }
                });
                let _ = wasm_bindgen_futures::JsFuture::from(wait).await;
            }
            if opened.get() != n {
                return;
            }
            let live: Live = {
                let opened = opened.clone();
                Rc::new(move || opened.get() == n)
            };
            let content = work(live).await;
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
    // While the results panel is in the page flow (a phone), a touch that starts outside the
    // model area keeps its browser action, so a swipe there scrolls the page. A touch on the
    // model keeps the gestures of the model. Each release stops the mouse events of the browser.
    let (touch_slot, on_model) = (slot.clone(), Cell::new(true));
    let options = eframe::WebOptions {
        depth_buffer: crate::gpu::DEPTH_BITS,
        should_prevent_default: Box::new(move |event| match event {
            egui::Event::PointerButton {
                pos, pressed: true, ..
            } => {
                let model = touch_slot.lock().ok().and_then(|r| r.model);
                on_model.set(model.is_none_or(|m| m.contains(*pos)));
                on_model.get()
            }
            egui::Event::PointerMoved(_) => on_model.get(),
            _ => true,
        }),
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
