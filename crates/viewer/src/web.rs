//! The browser entry point. The page of `fh serve` calls `start` with its canvas. The app
//! reads the token, the project and the run from the page address, and fetches the result
//! files from the daemon with `Authorization: Bearer <token>`. The chat panel sends the
//! same header with each request.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use eframe::egui;
use wasm_bindgen::{prelude::*, JsCast};

use crate::app::{Content, ContentSlot, ViewerApp};
use crate::chat::{Answer, Chat, Remote, Reply, Target};
use crate::{preview, results, RESULTS_FILE};

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

/// Fetch the result files of a run and read them, with the supports and loads of its input.
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
    // The record names the revision that the run solved.
    let path = format!("/api/v1/projects/{project}/runs/{run}");
    let record = match get(token, &path).await {
        Ok(bytes) => text(bytes, &path),
        Err(e) => Err(e),
    };
    let loads = match record.map(|r| Reply::parse(&r).fields.get("revision").cloned()) {
        Ok(Some(revision)) => model(token, project, &revision, run).await,
        Ok(None) => Err(format!("{path}: no revision")),
        Err(e) => Err(e),
    };
    Ok(Content::Results(Box::new(r), loads))
}

/// Fetch the input of a revision and read its supports and loads.
async fn model(
    token: &str,
    project: &str,
    revision: &str,
    name: &str,
) -> Result<Box<preview::Preview>, String> {
    let path = format!("/api/v1/projects/{project}/revisions/{revision}");
    let input = text(get(token, &path).await?, &path)?;
    let model = fh_inp::parse(&input).map_err(|e| format!("{name} ({revision}): {e}"))?;
    let mut p = preview::build(&model, &preview::units(&input));
    p.title = name.to_owned();
    Ok(Box::new(p))
}

/// The runs that have results, newest first, as links to open them.
async fn list_runs(token: &str) -> Result<Content, String> {
    let projects = text(get(token, "/api/v1/projects").await?, "/api/v1/projects")?;
    let mut links = Vec::new();
    for line in Reply::parse(&projects).items {
        let Some((project, root)) = line.split_once(' ') else {
            continue;
        };
        let path = format!("/api/v1/projects/{project}/runs");
        let runs = text(get(token, &path).await?, &path)?;
        for run in Reply::parse(&runs).items.iter().rev() {
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

/// The daemon of the page, through `fetch`.
struct Browser {
    token: String,
    ctx: egui::Context,
    answers: Rc<RefCell<VecDeque<Answer>>>,
}

impl Remote for Browser {
    fn request(&mut self, id: u64, method: &str, path: &str, body: &str) {
        let (token, method, path) = (self.token.clone(), method.to_owned(), path.to_owned());
        let body = (method != "GET").then(|| body.to_owned());
        let (answers, ctx) = (self.answers.clone(), self.ctx.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let answer = fetch(&token, &method, &path, body.as_deref()).await;
            let answer = answer.and_then(|bytes| text(bytes, &path));
            answers.borrow_mut().push_back((id, answer));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) -> Vec<Answer> {
        self.answers.borrow_mut().drain(..).collect()
    }
}

/// Start the viewer on a canvas of the page.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let slot: ContentSlot = Arc::new(Mutex::new(None));
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
                let mut app = ViewerApp::new(cc, title, first, Some(slot.clone()));
                if let Some(token) = token {
                    let remote = Browser {
                        token: token.clone(),
                        ctx: cc.egui_ctx.clone(),
                        answers: Rc::default(),
                    };
                    // The model before a solve and a run that succeeds during the chat open
                    // in the viewer.
                    let (t, ctx) = (token.clone(), cc.egui_ctx.clone());
                    let open = move |target: &Target| {
                        let (t, target, fill, ctx) =
                            (t.clone(), target.clone(), slot.clone(), ctx.clone());
                        wasm_bindgen_futures::spawn_local(async move {
                            let content = match &target {
                                Target::Run(p, r) => load_run(&t, p, r).await,
                                Target::Model(p, rev, name) => {
                                    model(&t, p, rev, name).await.map(Content::Preview)
                                }
                            };
                            if let Ok(mut s) = fill.lock() {
                                *s = Some(content.unwrap_or_else(Content::Failed));
                            }
                            ctx.request_repaint();
                        });
                    };
                    app.set_chat(Chat::new(Box::new(remote), Box::new(open)));
                }
                Ok(Box::new(app))
            }),
        )
        .await
}
