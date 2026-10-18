//! The browser entry point. The page of `fh serve` calls `start` with its canvas. The app
//! reads the token, the project and the run from the page address, and fetches the result
//! files from the daemon with `Authorization: Bearer <token>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use wasm_bindgen::{prelude::*, JsCast};

use crate::app::{Content, ContentSlot, ViewerApp};
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
    let window = web_sys::window().ok_or("no window")?;
    let init = web_sys::RequestInit::new();
    init.set_method("GET");
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
        // `results::load` refuses a file name outside the run folder before it reads.
        if e.file.contains("..") || e.file.contains('/') || e.file.contains('\\') {
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

/// Start the viewer on a canvas of the page.
#[wasm_bindgen]
pub fn start(canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let slot: ContentSlot = Arc::new(Mutex::new(None));
    let (title, first) = match query() {
        Err(e) => ("femahoy".to_owned(), Content::Failed(e)),
        Ok(q) => {
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
    wasm_bindgen_futures::spawn_local(async move {
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| Ok(Box::new(ViewerApp::new(cc, title, first, Some(slot))))),
            )
            .await;
        if let Err(e) = started {
            web_sys::console::error_1(&e);
        }
    });
    Ok(())
}
