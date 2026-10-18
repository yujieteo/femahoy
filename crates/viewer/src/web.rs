//! The browser entry point. The page of `fh serve` calls `start` with its canvas. The app
//! reads the token, the project and the run from the page address, and fetches the result
//! files from the daemon with `Authorization: Bearer <token>`. The terminal sidebar uses
//! the same token: in the header of each request, and as a subprotocol item of each
//! WebSocket, because a browser cannot set a header on a WebSocket.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use eframe::egui;
use wasm_bindgen::{prelude::*, JsCast};

use crate::app::{Content, ContentSlot, ViewerApp};
use crate::sidebar::{Remote, RemoteEvent, Sidebar};
use crate::terminal::{PROTOCOL, TOKEN_PROTOCOL};
use crate::{results, RESULTS_FILE};

/// The query of the page address: `token`, and `project` and `run` to open a run.
struct Query {
    token: String,
    project: Option<String>,
    run: Option<String>,
}

fn query() -> Result<Query, String> {
    let search = web_sys::window()
        .ok_or("no window")?
        .location()
        .search()
        .map_err(|_| "cannot read the page address")?;
    let params = web_sys::UrlSearchParams::new_with_str(&search)
        .map_err(|_| "cannot read the page address")?;
    let token = params
        .get("token")
        .ok_or("the page address has no token. Open the address that fh serve prints.")?;
    Ok(Query {
        token,
        project: params.get("project").filter(|p| !p.is_empty()),
        run: params.get("run").filter(|r| !r.is_empty()),
    })
}

/// GET a daemon path. An HTTP error gives the `error` and `message` fields of the answer.
async fn get(token: &str, path: &str) -> Result<Vec<u8>, String> {
    fetch(token, "GET", path, None).await
}

/// Send a request to the daemon, with the fields of a POST in `body`.
async fn fetch(
    token: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    if let Some(body) = body {
        init.set_body(&JsValue::from_str(body));
    }
    let request = web_sys::Request::new_with_str_and_init(path, &init)
        .map_err(|e| format!("{path}: {e:?}"))?;
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
            "{path}: HTTP {}: {} ({})",
            response.status(),
            field("message"),
            field("error")
        ));
    }
    Ok(bytes)
}

fn text(bytes: Vec<u8>, path: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8 text"))
}

/// The item lines of a daemon message (after the empty line that ends the fields).
fn items(message: &str) -> Vec<String> {
    message
        .split_once("\n\n")
        .map(|(_, rest)| rest.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Fetch the result files of a run and read them.
async fn load_run(token: &str, project: &str, run: &str) -> Result<Content, String> {
    let base = format!("/api/v1/projects/{project}/runs/{run}/files");
    let pvd_path = format!("{base}/{RESULTS_FILE}");
    let pvd = text(get(token, &pvd_path).await?, &pvd_path)?;
    let entries = fh_io::vtu::read_pvd(&pvd).map_err(|e| format!("{RESULTS_FILE}: {e}"))?;
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
    .map_err(|e| format!("Run {run} of project {project}: {e}"))?;
    Ok(Content::Results(Box::new(r)))
}

/// The runs that have results, newest first, as links to open them.
async fn list_runs(token: &str) -> Result<Content, String> {
    let projects = text(get(token, "/api/v1/projects").await?, "/api/v1/projects")?;
    let mut links = Vec::new();
    for line in items(&projects) {
        let Some((project, root)) = line.split_once(' ') else {
            continue;
        };
        let path = format!("/api/v1/projects/{project}/runs");
        let runs = text(get(token, &path).await?, &path)?;
        for run in items(&runs).iter().rev() {
            let Some((id, state)) = run.split_once(' ') else {
                continue;
            };
            if state == "succeeded" {
                let url = format!("?token={token}&project={project}&run={id}");
                links.push((format!("{root}: run {id}"), url));
            }
        }
    }
    Ok(Content::Runs(links))
}

/// The events of the browser callbacks, in their order, for the sidebar to read.
type Events = Rc<RefCell<VecDeque<RemoteEvent>>>;

fn push(events: &Events, ctx: &egui::Context, event: RemoteEvent) {
    events.borrow_mut().push_back(event);
    ctx.request_repaint();
}

/// The text for the person when the WebSocket of a terminal closes.
fn close_reason(code: u16, reason: &str) -> String {
    let why = match code {
        1000 => "the daemon closed the connection",
        1002 | 1003 | 1009 => "the daemon refused a message",
        1006 => "the connection failed. Make sure that fh serve runs",
        _ => "the connection closed",
    };
    match reason {
        "" => format!("{why} (code {code})"),
        _ => format!("{why}: {reason} (code {code})"),
    }
}

/// The WebSocket of a terminal and its callbacks, which must live as long as it.
struct Socket {
    ws: web_sys::WebSocket,
    _open: Closure<dyn FnMut()>,
    _message: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _close: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

impl Drop for Socket {
    fn drop(&mut self) {
        // A late event of this socket must not change a new socket of the same terminal.
        self.ws.set_onopen(None);
        self.ws.set_onmessage(None);
        self.ws.set_onclose(None);
        let _ = self.ws.close_with_code(1000);
    }
}

/// The daemon of the page, through `fetch` and WebSockets.
struct Browser {
    token: String,
    host: String,
    ctx: egui::Context,
    events: Events,
    sockets: HashMap<String, Socket>,
}

impl Remote for Browser {
    fn request(&mut self, id: u64, method: &str, path: &str, body: &str) {
        let (token, method, path) = (self.token.clone(), method.to_owned(), path.to_owned());
        let body = (method != "GET").then(|| body.to_owned());
        let (events, ctx) = (self.events.clone(), self.ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let answer = fetch(&token, &method, &path, body.as_deref()).await;
            let answer = answer.and_then(|bytes| text(bytes, &path));
            push(&events, &ctx, RemoteEvent::Answer(id, answer));
        });
    }

    fn connect(&mut self, terminal: &str) {
        self.sockets.remove(terminal);
        let id = terminal.to_owned();
        let url = format!("ws://{}/api/v1/terminals/{id}/ws", self.host);
        let token = format!("{TOKEN_PROTOCOL}{}", self.token);
        let protocols = js_sys::Array::of2(&PROTOCOL.into(), &token.into());
        let ws = match web_sys::WebSocket::new_with_str_sequence(&url, &protocols) {
            Ok(ws) => ws,
            Err(e) => {
                let why = format!("the browser cannot open the WebSocket: {e:?}");
                return push(&self.events, &self.ctx, RemoteEvent::Closed(id, why));
            }
        };
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);
        let (events, ctx, t) = (self.events.clone(), self.ctx.clone(), id.clone());
        let open = Closure::<dyn FnMut()>::new(move || {
            push(&events, &ctx, RemoteEvent::Opened(t.clone()));
        });
        let (events, ctx, t) = (self.events.clone(), self.ctx.clone(), id.clone());
        let message = Closure::<dyn FnMut(_)>::new(move |e: web_sys::MessageEvent| {
            // The daemon sends binary messages only. Other data is a bad message.
            let bytes = e.data().dyn_into::<js_sys::ArrayBuffer>();
            let bytes = bytes.map_or_else(|_| Vec::new(), |b| js_sys::Uint8Array::new(&b).to_vec());
            push(&events, &ctx, RemoteEvent::Message(t.clone(), bytes));
        });
        let (events, ctx, t) = (self.events.clone(), self.ctx.clone(), id.clone());
        let close = Closure::<dyn FnMut(_)>::new(move |e: web_sys::CloseEvent| {
            let why = close_reason(e.code(), &e.reason());
            push(&events, &ctx, RemoteEvent::Closed(t.clone(), why));
        });
        ws.set_onopen(Some(open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(close.as_ref().unchecked_ref()));
        let socket = Socket {
            ws,
            _open: open,
            _message: message,
            _close: close,
        };
        self.sockets.insert(id, socket);
    }

    fn send(&mut self, terminal: &str, message: &[u8]) {
        // A failed send closes the socket, and its close event tells the person.
        if let Some(s) = self.sockets.get(terminal) {
            let _ = s.ws.send_with_u8_array(message);
        }
    }

    fn disconnect(&mut self, terminal: &str) {
        self.sockets.remove(terminal);
    }

    fn poll(&mut self) -> Vec<RemoteEvent> {
        self.events.borrow_mut().drain(..).collect()
    }
}

/// Start the viewer on a canvas of the page.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let slot: ContentSlot = Arc::new(Mutex::new(None));
    let host = web_sys::window().and_then(|w| w.location().host().ok());
    let mut token = None;
    let (title, first) = match query() {
        Err(e) => ("femahoy".to_owned(), Content::Failed(e)),
        Ok(q) => {
            token = Some(q.token.clone());
            let fill = slot.clone();
            let title = match (&q.project, &q.run) {
                (Some(p), Some(r)) => format!("project {p}, run {r}"),
                _ => "femahoy".to_owned(),
            };
            wasm_bindgen_futures::spawn_local(async move {
                let content = match (&q.project, &q.run) {
                    (Some(p), Some(r)) => load_run(&q.token, p, r).await,
                    _ => list_runs(&q.token).await,
                };
                let content = content.unwrap_or_else(Content::Failed);
                if let Ok(mut s) = fill.lock() {
                    *s = Some(content);
                }
            });
            (title, Content::Loading("Loading the results...".into()))
        }
    };
    let options = eframe::WebOptions {
        depth_buffer: crate::gpu::DEPTH_BITS,
        ..Default::default()
    };
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(move |cc| {
                let mut app = ViewerApp::new(cc, title, first, Some(slot));
                if let (Some(token), Some(host)) = (token, host) {
                    let remote = Browser {
                        token,
                        host,
                        ctx: cc.egui_ctx.clone(),
                        events: Rc::default(),
                        sockets: HashMap::new(),
                    };
                    app.set_sidebar(Sidebar::new(Box::new(remote)));
                }
                Ok(Box::new(app))
            }),
        )
        .await
}
