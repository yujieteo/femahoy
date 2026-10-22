//! The HTTP server of the page, on 127.0.0.1 only (DESIGN.md section 4.1).
//!
//! # Access rules
//!
//! 1. The `Host` header must be `127.0.0.1:<port>`. This stops DNS rebinding.
//! 2. A request with an `Origin` header must come from the daemon's own page,
//!    `http://127.0.0.1:<port>`. Any other `Origin`, also `null`, fails with 403.
//! 3. Every request must carry `Authorization: Bearer <token>`. A client with no `Origin`
//!    needs only the token. Only `GET /`, the page of the daemon, can also
//!    take the token as `?token=<token>`, because a browser that opens a link sends no header.
//!
//! The daemon sends no CORS header, so a page of another origin cannot read an answer.
//! Each answer has `Referrer-Policy: no-referrer`, so the page never sends its address with
//! the token to another site.
//!
//! # Routes
//!
//! | Method and path | Body | Answer |
//! |---|---|---|
//! | `GET /` | | the page of the daemon (HTML): the chat, the list of the runs and the wasm viewer |
//! | `GET /viewer/fh_viewer.js`, `GET /viewer/fh_viewer_bg.wasm` | | the wasm viewer |
//! | `GET /api/v1/runs` | | one item `<run> <input>` for each run that has results |
//! | `GET /api/v1/runs/<r>/input` | | the text of the copy of the input that the run solves, with its `*INCLUDE` files (bytes). No bytes for an OpenFOAM, GetDP, preCICE, Chrono, LAMMPS or openEMS case |
//! | `GET /api/v1/runs/<r>/files/<name>` | | a file of the run folder (bytes) |
//! | `GET /api/v1/previews/<id>` | | the text of the copy of the input of the preview, with its `*INCLUDE` files (bytes) |
//! | `GET /api/v1/chat?since=N` | | `state` (`running` or `idle`), `next`, and while the agent works `started_ms`; one item `<seq> <kind> <text>` for each entry after `N` (`chat` module) |
//! | `POST /api/v1/chat` | the text of the message | `state`: the message goes to the agent, also while it works |
//! | `POST /api/v1/chat/stop` | | `state`: the agent and its processes stop |
//!
//! The page has no secret in it. Its script reads the token from the page address and
//! fetches the viewer and the results with the `Authorization` header, so only `GET /`
//! takes the token in the query. The page allows only its own script (a new nonce for each
//! answer), the viewer module, wasm, and fetches to its own origin.
//!
//! The run and preview routes only read. `fh run` writes those folders (`store` module).
//!
//! A failed request gets the HTTP status and the field `message`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use super::chat::Chat;
use super::message::{bad, fail, Failure, Message};
use super::store::Store;

const MAX_HEAD: usize = 16 << 10;
const MAX_HEADERS: usize = 64;
/// The most bytes of a body, for each route but the chat.
const MAX_BODY: usize = 16 << 10;
/// The most bytes of a chat message: a long pasted paper must fit.
const MAX_CHAT_BODY: usize = 1 << 20;
/// The most connections at the same time. Each one has a thread.
const MAX_CONNECTIONS: usize = 32;
/// The longest time that a client can take to send its request. A browser sends it at once.
const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// The longest time that a client can take to read a part of the answer.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// The page of the daemon (`page.html`): the chat on the left and the wasm viewer
/// (`crates/viewer`, `start`) in the center. `NONCE` is the nonce of its script.
const PAGE: &str = include_str!("page.html");

/// The files of the wasm viewer that the daemon serves, with their content types.
pub(super) const VIEWER_FILES: [(&str, &str); 2] = [
    ("fh_viewer.js", "text/javascript"),
    ("fh_viewer_bg.wasm", "application/wasm"),
];

/// The answer to a request that passed the access rules.
enum Reply {
    Message(Message),
    /// The page, and the nonce of its script.
    Page(String),
    /// File bytes and their content type.
    Bytes(&'static str, Vec<u8>),
}

struct Ctx {
    store: Arc<Store>,
    chat: Arc<Chat>,
    viewer: Option<PathBuf>,
    token: String,
}

pub(crate) struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// `viewer` is the folder with the wasm viewer files, or `None` when it is not built.
    pub(crate) fn start(
        store: Arc<Store>,
        chat: Arc<Chat>,
        token: String,
        viewer: Option<PathBuf>,
    ) -> io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let ctx = Arc::new(Ctx {
            store,
            chat,
            viewer,
            token,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
                let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let busy = fail(503, "too many connections");
                    let _ = respond(&mut stream, Err(busy));
                    continue;
                }
                let (ctx, active) = (ctx.clone(), active.clone());
                std::thread::spawn(move || {
                    // A browser opens spare connections. One that sends no byte before the
                    // timeout, or closes, gets no answer: the browser can send its next request
                    // on it and read a 400 that answers nothing.
                    if matches!(stream.peek(&mut [0]), Ok(1)) {
                        let answer = read_request(&mut stream).and_then(|req| {
                            authorize(&req, port, &ctx.token)?;
                            route(&ctx, &req)
                        });
                        let _ = respond(&mut stream, answer);
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Server {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn query_value(&self, key: &str) -> Option<&str> {
        self.query
            .split('&')
            .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
    }

    fn body_limit(&self) -> usize {
        if self.method == "POST" && self.path == "/api/v1/chat" {
            MAX_CHAT_BODY
        } else {
            MAX_BODY
        }
    }

    fn is_page(&self) -> bool {
        self.method == "GET" && self.path == "/"
    }

    fn query_number(&self, key: &str) -> Result<u64, Failure> {
        match self.query_value(key) {
            None => Ok(0),
            Some(v) => v.parse().map_err(|_| bad(format!("{key} is not a number"))),
        }
    }
}

fn read_line(reader: &mut impl BufRead, head: &mut usize) -> Result<String, Failure> {
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| bad(e.to_string()))?;
    *head += n;
    if *head > MAX_HEAD {
        return Err(fail(431, "the request head is too large"));
    }
    if n == 0 || !line.ends_with('\n') {
        return Err(bad("the request ended early"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Failure> {
    let mut reader = BufReader::new(Read::take(&*stream, (MAX_HEAD + MAX_CHAT_BODY + 1) as u64));
    let mut head = 0;
    let line = read_line(&mut reader, &mut head)?;
    let parts: Vec<&str> = line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(bad("bad request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(bad("only HTTP/1 is served"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let line = read_line(&mut reader, &mut head)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(bad("bad header line"));
        };
        let name = name.trim().to_ascii_lowercase();
        let unique = ["host", "origin", "authorization", "content-length"];
        if headers.len() == MAX_HEADERS
            || (unique.contains(&name.as_str()) && headers.iter().any(|(n, _)| *n == name))
        {
            return Err(bad(format!("bad header {name}")));
        }
        headers.push((name, value.trim().to_owned()));
    }
    let mut req = Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
        body: String::new(),
    };
    if req.header("transfer-encoding").is_some() {
        return Err(bad("send a Content-Length body"));
    }
    let length: usize = match req.header("content-length") {
        None => 0,
        Some(v) => v.parse().map_err(|_| bad("bad Content-Length"))?,
    };
    if length > req.body_limit() {
        return Err(fail(413, "the body is too large"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| bad(e.to_string()))?;
    req.body = String::from_utf8(body).map_err(|_| bad("body is not UTF-8"))?;
    Ok(req)
}

/// Compare in a time that does not depend on where the texts differ.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

fn authorize(req: &Request, port: u16, token: &str) -> Result<(), Failure> {
    let own = format!("127.0.0.1:{port}");
    if req.header("host") != Some(own.as_str()) {
        return Err(fail(403, format!("the Host must be {own}")));
    }
    if let Some(origin) = req.header("origin") {
        if origin != format!("http://{own}") {
            return Err(fail(403, format!("Origin {origin} is not the daemon page")));
        }
    }
    let given = match req.header("authorization") {
        Some(v) => v.strip_prefix("Bearer "),
        None if req.is_page() => req.query_value("token"),
        None => None,
    };
    let Some(given) = given else {
        return Err(fail(401, "send Authorization: Bearer <token>"));
    };
    if !same_token(given, token) {
        return Err(fail(401, "the token is not correct"));
    }
    Ok(())
}

/// A file of the wasm viewer. A daemon with no viewer folder, or a folder with no such
/// file, gives 404.
fn viewer_file(viewer: Option<&Path>, name: &str) -> Result<Reply, Failure> {
    let Some(&(file, kind)) = VIEWER_FILES.iter().find(|(f, _)| *f == name) else {
        return Err(fail(404, format!("no viewer file {name}")));
    };
    let missing = |why: String| {
        fail(
            404,
            format!("{why}. Build it with scripts/build-viewer.sh, or set FH_VIEWER"),
        )
    };
    let dir = viewer.ok_or_else(|| missing("the wasm viewer is not built".into()))?;
    let bytes = std::fs::read(dir.join(file))
        .map_err(|e| missing(format!("{}: {e}", dir.join(file).display())))?;
    Ok(Reply::Bytes(kind, bytes))
}

fn route(ctx: &Ctx, req: &Request) -> Result<Reply, Failure> {
    if req.is_page() {
        return Ok(Reply::Page(
            super::random_hex(16).map_err(|e| fail(500, e))?,
        ));
    }
    let store = &ctx.store;
    let segments: Vec<&str> = req.path.split('/').collect();
    let mut m = Message::new();
    match (req.method.as_str(), &segments[..]) {
        ("GET", ["", "viewer", name]) => return viewer_file(ctx.viewer.as_deref(), name),
        ("GET", ["", "api", "v1", "chat"]) => {
            m = ctx.chat.read(req.query_number("since")?);
        }
        ("POST", ["", "api", "v1", "chat"]) => {
            ctx.chat.ask(&req.body)?;
            m.set("state", "running");
        }
        ("POST", ["", "api", "v1", "chat", "stop"]) => {
            ctx.chat.stop();
            m.set("state", "idle");
        }
        ("GET", ["", "api", "v1", "runs"]) => {
            m.items = store
                .runs()?
                .iter()
                .map(|(run, input)| format!("{run} {input}"))
                .collect();
        }
        ("GET", ["", "api", "v1", "runs", r, "input"]) => {
            return Ok(Reply::Bytes(
                "application/octet-stream",
                store.input_file(r)?,
            ));
        }
        ("GET", ["", "api", "v1", "runs", r, "files", name]) => {
            return Ok(Reply::Bytes(
                "application/octet-stream",
                store.result_file(r, name)?,
            ));
        }
        ("GET", ["", "api", "v1", "previews", id]) => {
            return Ok(Reply::Bytes(
                "application/octet-stream",
                store.preview_file(id)?,
            ));
        }
        _ => return Err(fail(404, format!("no route {} {}", req.method, req.path))),
    }
    Ok(Reply::Message(m))
}

fn respond(stream: &mut TcpStream, answer: Result<Reply, Failure>) -> io::Result<()> {
    let text = "text/plain; charset=utf-8";
    let api_policy = "default-src 'none'; frame-ancestors 'none'".to_owned();
    let (status, kind, body, policy) = match answer {
        Ok(Reply::Message(m)) => (200, text, m.to_text().into_bytes(), api_policy),
        Ok(Reply::Page(nonce)) => (
            200,
            "text/html; charset=utf-8",
            PAGE.replace("NONCE", &nonce).into_bytes(),
            page_policy(&nonce),
        ),
        Ok(Reply::Bytes(kind, bytes)) => (200, kind, bytes, api_policy),
        Err((status, message)) => {
            let mut m = Message::new();
            m.set("message", message);
            (status, text, m.to_text().into_bytes(), api_policy)
        }
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: {policy}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

/// The content security policy of the page: its own script (by nonce), the viewer module
/// (a blob), wasm, inline styles (the page and egui), and fetches to its own origin.
fn page_policy(nonce: &str) -> String {
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}' blob: 'wasm-unsafe-eval'; \
         style-src 'unsafe-inline'; connect-src 'self'; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    )
}
