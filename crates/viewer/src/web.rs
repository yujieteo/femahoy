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
use js_sys::Uint8Array;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, AbortSignal};

use crate::app::{Content, Request, RequestSlot, ViewerApp};
use crate::{preview, results, view, RESULTS_FILE};

/// GET a daemon path. An HTTP error gives the `error` and `message` fields of the answer. An
/// abort of `signal` stops the download.
async fn get(token: &str, path: &str, signal: &AbortSignal) -> Result<Uint8Array, String> {
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_signal(Some(signal));
    let request = web_sys::Request::new_with_str_and_init(path, &init)
        .map_err(|e| format!("{path}: {e:?}"))?;
    request
        .headers()
        .set("Authorization", &format!("Bearer {token}"))
        .map_err(|e| format!("{path}: {e:?}"))?;
    let answer = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{path}: the daemon does not answer ({e:?})"))?;
    let response: web_sys::Response = answer.dyn_into().map_err(|_| "not a response")?;
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|e| format!("{path}: {e:?}"))?,
    )
    .await
    .map_err(|e| format!("{path}: {e:?}"))?;
    let bytes = Uint8Array::new(&buffer);
    if !response.ok() {
        let text = String::from_utf8_lossy(&bytes.to_vec()).into_owned();
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

fn text(bytes: Uint8Array, path: &str) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| format!("{path}: not UTF-8 text"))
}

/// The text of a load that a later load replaced. The viewer never shows it.
const REPLACED: &str = "replaced by a later load";

/// The daemon path of the viewer script. A report answers it from its own files.
const SCRIPT_PATH: &str = "/viewer/fh_viewer.js";

/// The script of the worker that reads the steps of a run with [`StepReader`]. Its first
/// message has the text of the viewer script, the compiled viewer and the text of `results.pvd`.
/// The worker imports the script from its own blob. Reason: a report from `file://` has an
/// opaque origin, so a worker cannot import a blob of the page, and a module worker does not
/// start. Each next
/// message has the bytes of the next step, or the error of its fetch. Each message gets 1
/// answer: the read step in `ok` and the file of the next step in `next`, or an `error`.
const STEP_WORKER: &str = r#"
let reader;
onmessage = async ({ data }) => {
  try {
    if (data.script) {
      const viewer = await import(URL.createObjectURL(new Blob([data.script], { type: "text/javascript" })));
      await viewer.default({ module_or_path: data.module });
      reader = new viewer.StepReader(data.pvd);
      postMessage({ next: reader.next() });
    } else {
      const ok = reader.add(data.bytes ?? new Uint8Array(), data.error);
      postMessage({ ok, next: reader.next() }, [ok.buffer]);
    }
  } catch (e) {
    postMessage({ error: e instanceof Error ? e.message : String(e) });
  }
};
"#;

thread_local! {
    /// The blob URL of [`STEP_WORKER`]. Each load starts a worker from it.
    static STEP_WORKER_URL: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Reads the steps of a run in the worker of [`load_run`], with [`results::Loader`]. Only that
/// worker uses it.
#[wasm_bindgen]
pub struct StepReader(results::Loader);

#[wasm_bindgen]
impl StepReader {
    #[wasm_bindgen(constructor)]
    pub fn new(pvd: &str) -> Result<StepReader, String> {
        results::Loader::new(RESULTS_FILE, pvd).map(StepReader)
    }

    /// The file of the next step, or none when each step is read.
    pub fn next(&self) -> Option<String> {
        self.0.next_file().map(str::to_owned)
    }

    /// Read the next step from its `bytes`, or from the `error` of its fetch.
    pub fn add(&mut self, bytes: Vec<u8>, error: Option<String>) -> Result<Vec<u8>, String> {
        self.0.add_and_send(error.map_or(Ok(bytes), Err))
    }
}

/// The worker of 1 load. It stops when it is dropped, also in the middle of a step.
struct StepWorker(web_sys::Worker);

impl Drop for StepWorker {
    fn drop(&mut self) {
        self.0.terminate();
    }
}

impl StepWorker {
    fn start() -> Result<StepWorker, String> {
        let url = STEP_WORKER_URL.with(|u| -> Result<String, JsValue> {
            if let Some(url) = u.borrow().as_ref() {
                return Ok(url.clone());
            }
            let kind = web_sys::BlobPropertyBag::new();
            kind.set_type("text/javascript");
            let parts = js_sys::Array::of1(&STEP_WORKER.into());
            let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &kind)?;
            let url = web_sys::Url::create_object_url_with_blob(&blob)?;
            *u.borrow_mut() = Some(url.clone());
            Ok(url)
        });
        url.and_then(|url| web_sys::Worker::new(&url))
            .map(StepWorker)
            .map_err(|e| format!("the step reader does not start ({e:?})"))
    }

    /// Send `message` with the buffers `transfer`, and wait for its answer. An abort of
    /// `signal` stops the wait.
    async fn ask(
        &self,
        message: &js_sys::Object,
        transfer: &js_sys::Array,
        signal: &AbortSignal,
    ) -> Result<JsValue, String> {
        let answer = js_sys::Promise::new(&mut |done, fail| {
            self.0.set_onmessage(Some(&done));
            self.0.set_onerror(Some(&fail));
            signal.set_onabort(Some(&fail));
        });
        self.0
            .post_message_with_transfer(message, transfer)
            .map_err(|e| format!("the step reader: {e:?}"))?;
        let event = JsFuture::from(answer).await;
        signal.set_onabort(None);
        if signal.aborted() {
            return Err(REPLACED.into());
        }
        let field = |v: &JsValue, key: &str| js_sys::Reflect::get(v, &key.into()).ok();
        let data = event
            .and_then(|e| js_sys::Reflect::get(&e, &"data".into()))
            .map_err(|e| {
                let message = field(&e, "message").and_then(|m| m.as_string());
                format!("the step reader stopped: {}", message.unwrap_or_default())
            })?;
        match field(&data, "error").and_then(|e| e.as_string()) {
            Some(e) => Err(e),
            None => Ok(data),
        }
    }
}

/// Fetch the result files of a run and read them, with the supports and loads of its input and
/// the view `view` (the text of [`view::View::parse`]). A worker reads each step, so that a
/// large step does not stop the page. An abort of `signal` stops the download and the worker.
async fn load_run(
    token: &str,
    run: &str,
    view: &str,
    signal: &AbortSignal,
) -> Result<Content, String> {
    let view = view::View::parse(view).map_err(|e| format!("Run {run}: the view: {e}"))?;
    let base = format!("/api/v1/runs/{run}/files");
    let pvd_path = format!("{base}/{RESULTS_FILE}");
    let pvd = text(get(token, &pvd_path, signal).await?, &pvd_path)?;
    let script = text(get(token, SCRIPT_PATH, signal).await?, SCRIPT_PATH)?;
    let worker = StepWorker::start().map_err(|e| format!("Run {run}: {e}"))?;
    let set = |m: &js_sys::Object, key: &str, value: &JsValue| {
        let _ = js_sys::Reflect::set(m, &key.into(), value);
    };
    let first = js_sys::Object::new();
    set(&first, "script", &script.into());
    set(&first, "module", &wasm_bindgen::module());
    set(&first, "pvd", &pvd.into());
    let mut answer = worker.ask(&first, &js_sys::Array::new(), signal).await;
    let mut results = results::Results::default();
    loop {
        let reply = answer.map_err(|e| format!("Run {run}: {e}"))?;
        if let Some(ok) = js_sys::Reflect::get(&reply, &"ok".into())
            .ok()
            .and_then(|ok| ok.dyn_into::<Uint8Array>().ok())
        {
            results
                .receive(&ok.to_vec())
                .map_err(|e| format!("Run {run}: {e}"))?;
        }
        let Some(file) = js_sys::Reflect::get(&reply, &"next".into())
            .ok()
            .and_then(|n| n.as_string())
        else {
            break;
        };
        let path = format!("{base}/{file}");
        let (message, transfer) = (js_sys::Object::new(), js_sys::Array::new());
        match get(token, &path, signal).await {
            Ok(bytes) => {
                transfer.push(&bytes.buffer());
                set(&message, "bytes", &bytes);
            }
            Err(e) => set(&message, "error", &e.into()),
        }
        if signal.aborted() {
            return Err(REPLACED.into());
        }
        answer = worker.ask(&message, &transfer, signal).await;
    }
    drop(worker);
    let loads = model(token, &format!("/api/v1/runs/{run}/input"), run, signal).await;
    Ok(Content::Results(Box::new(results), loads, view))
}

/// Fetch an input file from the daemon path `path` and read its supports and loads.
async fn model(
    token: &str,
    path: &str,
    name: &str,
    signal: &AbortSignal,
) -> Result<Box<preview::Preview>, String> {
    let input = text(get(token, path, signal).await?, path)?;
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
    /// The abort of the last load. A later load aborts it, so the replaced load stops and shows
    /// nothing.
    abort: RefCell<Option<AbortController>>,
}

#[wasm_bindgen]
impl Viewer {
    /// Show the results of a run in place of the current content, with the view of a report.
    /// With no view, the panel shows each control that applies and the important fields of the run.
    pub fn open(&self, run: String, view: Option<String>) {
        self.ask(|r| r.mesh = Some(false));
        let token = self.token.clone();
        self.load("Loading the results...", move |signal| async move {
            load_run(&token, &run, view.as_deref().unwrap_or(""), &signal).await
        });
    }

    /// Open only the undeformed mesh, with no contour or independent scale.
    pub fn mesh(&self, run: String) {
        self.ask(|r| r.mesh = Some(true));
        let token = self.token.clone();
        self.load("Loading the mesh...", move |signal| async move {
            load_run(&token, &run, view::MESH_VIEW, &signal).await
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
        self.load("Loading the model...", move |signal| async move {
            let path = format!("/api/v1/previews/{preview}");
            model(&token, &path, &name, &signal)
                .await
                .map(Content::Preview)
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
        work: impl FnOnce(AbortSignal) -> W + 'static,
    ) {
        let fill = |slot: &RequestSlot, ctx: &egui::Context, content| {
            if let Ok(mut s) = slot.lock() {
                s.content = Some(content);
            }
            ctx.request_repaint();
        };
        // A step that the page asked for before this load is for the run before.
        self.ask(|r| r.step = None);
        let controller = AbortController::new();
        if let Some(replaced) = self.abort.replace(controller.as_ref().ok().cloned()) {
            replaced.abort();
        }
        let signal = match controller {
            Ok(c) => c.signal(),
            Err(e) => return fill(&self.slot, &self.ctx, Content::Failed(format!("{e:?}"))),
        };
        fill(&self.slot, &self.ctx, Content::Text(loading.into()));
        let (slot, ctx) = (self.slot.clone(), self.ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            // Start the fetch only after a frame took the text and freed the shown model.
            // Reason: on localhost the fetch and the parse end before the next frame, and the
            // page then holds two A320 models.
            while !signal.aborted() && slot.lock().is_ok_and(|s| s.content.is_some()) {
                let wait = js_sys::Promise::new(&mut |done, _| {
                    if let Some(w) = web_sys::window() {
                        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&done, 16);
                    }
                });
                let _ = JsFuture::from(wait).await;
            }
            if signal.aborted() {
                return;
            }
            let content = work(signal.clone()).await;
            if !signal.aborted() {
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
        abort: RefCell::default(),
    })
}
