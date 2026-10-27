//! The browser entry point. The page of `fh serve` (`crates/fh/src/daemon/page.html`) calls
//! `start` with its canvas and the token, and `Viewer::open` for each run that it shows.
//! The viewer fetches the result files from the daemon with `Authorization: Bearer
//! <token>`. A report (`crates/fh/src/daemon/report.html`) also gives the view of a run that
//! the agent writes to `Viewer::open`, the step of a slider to `Viewer::step` and its switch
//! "Show all controls" to `Viewer::all`. The page and a report give the text of a `grid.toml`
//! case to `Viewer::solve`, which solves it in a worker with no `fh run`, and `Viewer::stop`
//! stops that solve.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use eframe::egui;
use js_sys::Uint8Array;
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::{AbortController, AbortSignal};

use crate::app::{Content, Request, RequestSlot, ViewerApp};
use crate::{preview, results, solve, view, RESULTS_FILE};

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
/// start. Each next message has the bytes of the next step, or the error of its fetch. Each
/// message gets 1 answer: the byte lists of the read step in `ok`, with each buffer
/// transferred, and the file of the next step in `next`, or an `error`.
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
      postMessage({ ok, next: reader.next() }, ok.map((list) => list.buffer));
    }
  } catch (e) {
    postMessage({ error: e instanceof Error ? e.message : String(e) });
  }
};
"#;

/// The script of the worker that solves a `grid.toml` case with [`solve_grid`]. Its 1 message
/// has the text of the viewer script, the compiled viewer and the text of the case. It imports
/// the script from its own blob, as [`STEP_WORKER`] does. It answers with a `line` for each line
/// of progress, the byte lists of each output in `ok`, with each buffer transferred, and at the
/// end the probe table in `probes` with the bytes of its wasm memory in `memory`, or an `error`.
const SOLVE_WORKER: &str = r#"
onmessage = async ({ data }) => {
  try {
    const viewer = await import(URL.createObjectURL(new Blob([data.script], { type: "text/javascript" })));
    const wasm = await viewer.default({ module_or_path: data.module });
    const probes = viewer.solve_grid(data.text, (line) => postMessage({ line }),
      (ok) => postMessage({ ok }, ok.map((list) => list.buffer)));
    postMessage({ probes, memory: wasm.memory.buffer.byteLength });
  } catch (e) {
    // A trap of wasm (RuntimeError) is most often a failed memory growth.
    const text = e instanceof Error ? e.message : String(e);
    postMessage({ error: e instanceof WebAssembly.RuntimeError
      ? "the solve stopped in the browser (" + text + "), most often because the case needs more memory than the browser gives. Use a larger max_cell, or solve the case with fh run"
      : text });
  }
};
"#;

thread_local! {
    /// The blob URL of each worker script. Each load starts a worker from it.
    static WORKER_URLS: RefCell<Vec<(&'static str, String)>> = const { RefCell::new(Vec::new()) };
}

/// A worker of the script `source`, from its blob URL.
fn start_worker(source: &'static str) -> Result<web_sys::Worker, JsValue> {
    let url = WORKER_URLS.with(|urls| -> Result<String, JsValue> {
        if let Some((_, url)) = urls.borrow().iter().find(|(s, _)| std::ptr::eq(*s, source)) {
            return Ok(url.clone());
        }
        let kind = web_sys::BlobPropertyBag::new();
        kind.set_type("text/javascript");
        let parts = js_sys::Array::of1(&source.into());
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &kind)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        urls.borrow_mut().push((source, url.clone()));
        Ok(url)
    })?;
    web_sys::Worker::new(&url)
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
    pub fn add(&mut self, bytes: Vec<u8>, error: Option<String>) -> Result<js_sys::Array, String> {
        let lists = self.0.add_and_send(error.map_or(Ok(bytes), Err))?;
        Ok(lists.iter().map(|l| Uint8Array::from(&l[..])).collect())
    }
}

/// The byte lists of a step that the worker transferred.
impl results::Lists for js_sys::Array {
    fn size(&self, i: usize) -> Option<usize> {
        let list = self.get(i as u32).dyn_into::<Uint8Array>().ok()?;
        Some(list.length() as usize)
    }

    fn copy(&self, i: usize, out: &mut [u8]) {
        self.get(i as u32)
            .unchecked_into::<Uint8Array>()
            .copy_to(out);
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
        start_worker(STEP_WORKER)
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
            .and_then(|ok| ok.dyn_into::<js_sys::Array>().ok())
        {
            results
                .receive(&ok)
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

/// Solve a `grid.toml` case in the worker of the page, in the main thread only.
#[wasm_bindgen]
pub fn solve_grid(
    text: &str,
    on_line: &js_sys::Function,
    on_step: &js_sys::Function,
) -> Result<String, String> {
    solve::solve(
        text,
        solve::BROWSER,
        &mut |line| {
            let _ = on_line.call1(&JsValue::NULL, &line.into());
        },
        &mut |lists| {
            let lists: js_sys::Array = lists.iter().map(|l| Uint8Array::from(&l[..])).collect();
            let _ = on_step.call1(&JsValue::NULL, &lists);
        },
    )
}

/// The messages of a worker that the page did not read yet, and the wake of the read that
/// waits for the next one.
#[derive(Default)]
struct Inbox {
    queue: VecDeque<Result<JsValue, String>>,
    wake: Option<js_sys::Function>,
}

/// The worker of 1 browser solve. Its messages go to a queue, so that none is lost while the
/// page reads the one before. It stops when it is dropped, also in the middle of the solve.
struct SolveWorker {
    worker: web_sys::Worker,
    inbox: Rc<RefCell<Inbox>>,
    _handlers: [Closure<dyn FnMut(JsValue)>; 2],
}

impl Drop for SolveWorker {
    fn drop(&mut self) {
        self.worker.terminate();
    }
}

impl SolveWorker {
    fn start() -> Result<SolveWorker, String> {
        let worker = start_worker(SOLVE_WORKER)
            .map_err(|e| format!("the browser solver does not start ({e:?})"))?;
        let inbox: Rc<RefCell<Inbox>> = Rc::default();
        let handler = |error: bool| {
            let inbox = inbox.clone();
            Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
                let field = |key: &str| js_sys::Reflect::get(&event, &key.into()).ok();
                let message = match error {
                    false => field("data").ok_or_else(|| "a message with no data".to_owned()),
                    true => Err(format!(
                        "the browser solver stopped: {}",
                        field("message")
                            .and_then(|m| m.as_string())
                            .unwrap_or_default()
                    )),
                };
                let mut inbox = inbox.borrow_mut();
                inbox.queue.push_back(message);
                if let Some(wake) = inbox.wake.take() {
                    let _ = wake.call0(&JsValue::NULL);
                }
            })
        };
        let handlers = [handler(false), handler(true)];
        worker.set_onmessage(Some(handlers[0].as_ref().unchecked_ref()));
        worker.set_onerror(Some(handlers[1].as_ref().unchecked_ref()));
        Ok(SolveWorker {
            worker,
            inbox,
            _handlers: handlers,
        })
    }

    /// The next message of the worker. An abort of `signal` stops the wait.
    async fn next(&self, signal: &AbortSignal) -> Result<JsValue, String> {
        loop {
            if signal.aborted() {
                return Err(REPLACED.into());
            }
            if let Some(message) = self.inbox.borrow_mut().queue.pop_front() {
                return message;
            }
            let wait = js_sys::Promise::new(&mut |done, _| {
                signal.set_onabort(Some(&done));
                self.inbox.borrow_mut().wake = Some(done);
            });
            let _ = JsFuture::from(wait).await;
            signal.set_onabort(None);
        }
    }
}

/// Solve the case `text` (the text of `grid.toml`) in a worker. `on_event` gets
/// (`"line"`, each line of progress), and at the end (`"probes"`, the probe table). `show` puts
/// the last line in the viewer. An abort of `signal` stops the solve.
async fn solve_case(
    token: &str,
    text: String,
    name: &str,
    on_event: &js_sys::Function,
    show: &dyn Fn(String),
    signal: &AbortSignal,
) -> Result<Content, String> {
    let event = |kind: &str, text: &str| {
        let _ = on_event.call2(&JsValue::NULL, &kind.into(), &text.into());
    };
    // A file with an error gets its message at once, with no worker.
    if fh_core::grid::is_line(&text) {
        fh_core::line::read(&text).map(|_| ())
    } else {
        crate::solve::read_heat(&text).map(|_| ())
    }
    .map_err(|e| format!("{name}: {e}"))?;
    let script = self::text(get(token, SCRIPT_PATH, signal).await?, SCRIPT_PATH)?;
    let worker = SolveWorker::start()?;
    let message = js_sys::Object::new();
    for (key, value) in [
        ("script", script.into()),
        ("module", wasm_bindgen::module()),
        ("text", text.into()),
    ] {
        let _ = js_sys::Reflect::set(&message, &key.into(), &value);
    }
    worker
        .worker
        .post_message(&message)
        .map_err(|e| format!("the browser solver: {e:?}"))?;
    let mut results = results::Results::default();
    loop {
        let data = worker.next(signal).await?;
        let field = |key: &str| js_sys::Reflect::get(&data, &key.into()).ok();
        if let Some(e) = field("error").and_then(|e| e.as_string()) {
            return Err(format!("{name}: {e}"));
        }
        if let Some(line) = field("line").and_then(|l| l.as_string()) {
            event("line", &line);
            show(format!("Solving {name} in the browser: {line}"));
        }
        if let Some(ok) = field("ok").and_then(|ok| ok.dyn_into::<js_sys::Array>().ok()) {
            results.receive(&ok).map_err(|e| format!("{name}: {e}"))?;
        }
        if let Some(probes) = field("probes").and_then(|p| p.as_string()) {
            let memory = field("memory").and_then(|m| m.as_f64()).unwrap_or(0.0);
            event(
                "line",
                &format!(
                    "the solve used {:.0} MiB of browser memory",
                    memory / f64::from(1 << 20)
                ),
            );
            event("probes", &probes);
            break;
        }
    }
    drop(worker);
    let mut loads = preview::build(&fh_core::inp::parse(""), "");
    loads.title = name.to_owned();
    let view = view::View::parse("").map_err(|e| format!("{name}: the view: {e}"))?;
    Ok(Content::Results(
        Box::new(results),
        Ok(Box::new(loads)),
        view,
    ))
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

/// The callback of a browser solve and its last event, which it gets when this is dropped.
struct Last(js_sys::Function, RefCell<(&'static str, String)>);

impl Drop for Last {
    fn drop(&mut self) {
        let (kind, text) = &*self.1.borrow();
        let _ = self
            .0
            .call2(&JsValue::NULL, &(*kind).into(), &text.as_str().into());
    }
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

    /// Solve the case `text` (the text of a `grid.toml`, from the file `name`) in a worker, with
    /// no `fh run`, and show its results. `on_event(kind, text)` gets (`"line"`, each line of
    /// progress) and (`"probes"`, the probe table), and last 1 of (`"end"`, `""`), (`"error"`,
    /// why the solve failed) and (`"stopped"`, `""`) when [`Viewer::stop`] or a later load
    /// stopped it.
    pub fn solve(&self, text: String, name: String, on_event: js_sys::Function) {
        let token = self.token.clone();
        let (slot, ctx) = (self.slot.clone(), self.ctx.clone());
        // The last event goes when the work ends, also when a later load drops it before it
        // starts.
        let last = Last(on_event, RefCell::new(("stopped", String::new())));
        self.load(
            &format!("Solving {name} in the browser..."),
            move |signal| async move {
                let show = |text: String| {
                    if let Ok(mut s) = slot.lock() {
                        s.content = Some(Content::Text(text));
                    }
                    ctx.request_repaint();
                };
                let done = solve_case(&token, text, &name, &last.0, &show, &signal).await;
                match &done {
                    Ok(_) => *last.1.borrow_mut() = ("end", String::new()),
                    Err(e) if e == REPLACED => {}
                    Err(e) => *last.1.borrow_mut() = ("error", e.clone()),
                }
                done
            },
        );
    }

    /// Stop the solve of [`Viewer::solve`], and free its worker and its memory.
    pub fn stop(&self) {
        if let Some(solve) = self.abort.take() {
            solve.abort();
        }
        if let Ok(mut s) = self.slot.lock() {
            s.content = Some(Content::Text("The browser solve stopped.".into()));
        }
        self.ctx.request_repaint();
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
