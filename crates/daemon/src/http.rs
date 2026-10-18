//! The HTTP transport of the run contract, on 127.0.0.1 only (DESIGN.md section 4.1).
//!
//! # Access rules
//!
//! 1. The `Host` header must be `127.0.0.1:<port>`. This stops DNS rebinding.
//! 2. A request with an `Origin` header must come from the daemon's own page,
//!    `http://127.0.0.1:<port>`. Any other `Origin`, also `null`, fails with 403.
//! 3. Every request must carry `Authorization: Bearer <token>`. A client with no `Origin`,
//!    such as the CLI, needs only the token. Only `GET /`, the page of the daemon, can also
//!    take the token as `?token=<token>`, because a browser that opens a link sends no header.
//!    Only the WebSocket route of a terminal can also take the token as the subprotocol item
//!    `fh.token.<token>`, because a browser cannot set a header on a WebSocket.
//!
//! The terminal and pane routes have the same rules. A terminal is a real shell on this
//! machine, so these rules are its only way in.
//!
//! The daemon sends no CORS header, so a page of another origin cannot read an answer.
//! Each answer has `Referrer-Policy: no-referrer`, so the page never sends its address with
//! the token to another site.
//!
//! # Routes
//!
//! | Method and path | Body | Answer |
//! |---|---|---|
//! | `GET /` | | the page of the daemon (HTML): it loads the wasm viewer |
//! | `GET /viewer/fh_viewer.js`, `GET /viewer/fh_viewer_bg.wasm` | | the wasm viewer |
//! | `GET /api/v1/status` | | `instance`, `pid` |
//! | `GET /api/v1/projects` | | one item `<project> <root>` for each project |
//! | `POST /api/v1/projects` | `root` | `project`, `root` |
//! | `GET /api/v1/projects/<p>/runs` | | one item `<run> <state>` for each run |
//! | `POST /api/v1/projects/<p>/runs` | `request`, `input` | acknowledgement |
//! | `GET /api/v1/projects/<p>/runs/<r>` | | run record |
//! | `POST /api/v1/projects/<p>/runs/<r>/cancel` | `request` | acknowledgement |
//! | `GET /api/v1/projects/<p>/runs/<r>/events?since=N&wait_ms=T` | | events |
//! | `GET /api/v1/projects/<p>/runs/<r>/files/<name>` | | a file of the run folder (bytes) |
//! | `GET /api/v1/terminals` | | `panes`; one item `<t> <kind> <state> <title>` for each terminal |
//! | `POST /api/v1/terminals` | `kind` (`shell`) | terminal record |
//! | `GET /api/v1/terminals/<t>` | | terminal record |
//! | `GET /api/v1/terminals/<t>/text?lines=N` | | one item for each line of text |
//! | `POST /api/v1/terminals/<t>/close` | | terminal record |
//! | `GET /api/v1/terminals/<t>/ws` | | WebSocket upgrade, subprotocol `fh.term.v1` (`docs/terminal.md`) |
//! | `GET /api/v1/panes` | | `backend`, `detail`; one item `<pane> <state> <t or -> <title>` for each pane |
//! | `POST /api/v1/panes` | `title`, optional `cwd`, `command` | pane record |
//! | `GET /api/v1/panes/<pane>` | | pane record |
//! | `GET /api/v1/panes/<pane>/text?lines=N` | | one item for each line of text |
//! | `POST /api/v1/panes/<pane>/send` | optional `text`, `keys` (names, space between) | |
//! | `POST /api/v1/panes/<pane>/close` | | |
//!
//! The page has no secret in it. Its script reads the token from the page address and
//! fetches the viewer and the results with the `Authorization` header, so only `GET /`
//! takes the token in the query. The page allows only its own script (a new nonce for each
//! answer), the viewer module, wasm, and fetches to its own origin.
//!
//! An error has the fields `error` (a stable code) and `message`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::contract::{Error, Message};
use crate::panes::{valid_pane, Key, Pane, PaneSpec, Sidebar};
use crate::pty::{Terminal, HISTORY};
use crate::screen::{PROTOCOL, TOKEN_PROTOCOL};
use crate::service::Service;
use crate::ws;

const MAX_HEAD: usize = 16 << 10;
const MAX_HEADERS: usize = 64;
const MAX_BODY: usize = 16 << 10;
/// The most connections at the same time. Each one has a thread.
const MAX_CONNECTIONS: usize = 32;
const MAX_SOCKETS: usize = 16;
/// The longest time that a client can take to send its request.
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// The page of the daemon. `NONCE` is the nonce of its script. The script loads the wasm
/// viewer (`crates/viewer`, `start`) with the token from the page address.
const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>femahoy</title>
<style>
html, body { margin: 0; height: 100%; overflow: hidden; background: #1b1b1b; color: #ddd; font: 15px system-ui, sans-serif; }
#fh { display: block; width: 100%; height: 100%; }
#msg { position: absolute; top: 40%; width: 100%; text-align: center; padding: 0 16px; box-sizing: border-box; }
</style>
</head>
<body>
<canvas id="fh"></canvas>
<p id="msg">Loading the viewer...</p>
<script type="module" nonce="NONCE">
const msg = document.getElementById("msg");
const token = new URLSearchParams(location.search).get("token") || "";
async function get(path) {
  const r = await fetch(path, { headers: { Authorization: "Bearer " + token } });
  if (!r.ok) {
    const text = await r.text();
    const field = (k) => (text.split("\n").find((l) => l.startsWith(k + "=")) || "").slice(k.length + 1);
    throw new Error(path + ": HTTP " + r.status + ": " + field("message") + " (" + field("error") + ")");
  }
  return r;
}
try {
  const js = await (await get("/viewer/fh_viewer.js")).text();
  const viewer = await import(URL.createObjectURL(new Blob([js], { type: "text/javascript" })));
  const wasm = await (await get("/viewer/fh_viewer_bg.wasm")).arrayBuffer();
  await viewer.default({ module_or_path: wasm });
  await viewer.start(document.getElementById("fh"));
  msg.remove();
} catch (e) {
  msg.textContent = "The viewer cannot start. " + ((e && e.message) || e);
}
</script>
</body>
</html>
"#;

/// The files of the wasm viewer that the daemon serves, with their content types.
const VIEWER_FILES: [(&str, &str); 2] = [
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
    Upgrade(Arc<Terminal>, String),
}

struct Ctx {
    service: Arc<Service>,
    sidebar: Arc<Sidebar>,
    viewer: Option<PathBuf>,
    token: String,
    sockets: AtomicUsize,
}

pub(crate) struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// `viewer` is the folder with the wasm viewer files, or `None` when it is not built.
    pub(crate) fn start(
        service: Arc<Service>,
        sidebar: Arc<Sidebar>,
        port: u16,
        token: String,
        viewer: Option<PathBuf>,
    ) -> io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
        let port = listener.local_addr()?.port();
        let ctx = Arc::new(Ctx {
            service,
            sidebar,
            viewer,
            token,
            sockets: AtomicUsize::new(0),
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
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    let busy = Error::new(503, "daemon.busy", "too many connections");
                    let _ = respond(&mut stream, Err(busy), port);
                    continue;
                }
                let (ctx, active) = (ctx.clone(), active.clone());
                std::thread::spawn(move || {
                    let answer = read_request(&mut stream).and_then(|req| {
                        authorize(&req, port, &ctx.token)?;
                        match req.socket_terminal() {
                            Some(t) => upgrade(&ctx, &req, t),
                            None => route(&ctx, &req),
                        }
                    });
                    match answer {
                        Ok(Reply::Upgrade(t, accept)) => {
                            if switch(&mut stream, &accept).is_ok() {
                                ws::attach(stream, t);
                            }
                            ctx.sockets.fetch_sub(1, Ordering::SeqCst);
                        }
                        answer => drop(respond(&mut stream, answer, port)),
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

    fn is_page(&self) -> bool {
        self.method == "GET" && self.path == "/"
    }

    fn socket_terminal(&self) -> Option<&str> {
        let s: Vec<&str> = self.path.split('/').collect();
        match (self.method.as_str(), &s[..]) {
            ("GET", ["", "api", "v1", "terminals", t, "ws"]) => Some(t),
            _ => None,
        }
    }

    fn words(&self, name: &str) -> impl Iterator<Item = &str> {
        self.header(name).unwrap_or("").split(',').map(str::trim)
    }

    fn query_number(&self, key: &str) -> Result<u64, Error> {
        match self.query_value(key) {
            None => Ok(0),
            Some(v) => v
                .parse()
                .map_err(|_| Error::bad("query.invalid", format!("{key} is not a number"))),
        }
    }
}

fn read_line(reader: &mut impl BufRead, head: &mut usize) -> Result<String, Error> {
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .map_err(|e| Error::bad("http.invalid", e.to_string()))?;
    *head += n;
    if *head > MAX_HEAD {
        return Err(Error::new(
            431,
            "http.too-large",
            "the request head is too large",
        ));
    }
    if n == 0 || !line.ends_with('\n') {
        return Err(Error::bad("http.invalid", "the request ended early"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Error> {
    let mut reader = BufReader::new(Read::take(&*stream, (MAX_HEAD + MAX_BODY + 1) as u64));
    let mut head = 0;
    let line = read_line(&mut reader, &mut head)?;
    let parts: Vec<&str> = line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(Error::bad("http.invalid", "bad request line"));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(Error::bad("http.invalid", "only HTTP/1 is served"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let line = read_line(&mut reader, &mut head)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::bad("http.invalid", "bad header line"));
        };
        let name = name.trim().to_ascii_lowercase();
        let unique = [
            "host",
            "origin",
            "authorization",
            "content-length",
            "upgrade",
            "sec-websocket-key",
            "sec-websocket-protocol",
            "sec-websocket-version",
        ];
        if headers.len() == MAX_HEADERS
            || (unique.contains(&name.as_str()) && headers.iter().any(|(n, _)| *n == name))
        {
            return Err(Error::bad("http.invalid", format!("bad header {name}")));
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
        return Err(Error::bad("http.invalid", "send a Content-Length body"));
    }
    let length: usize = match req.header("content-length") {
        None => 0,
        Some(v) => v
            .parse()
            .map_err(|_| Error::bad("http.invalid", "bad Content-Length"))?,
    };
    if length > MAX_BODY {
        return Err(Error::new(413, "http.too-large", "the body is too large"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| Error::bad("http.invalid", e.to_string()))?;
    req.body =
        String::from_utf8(body).map_err(|_| Error::bad("http.invalid", "body is not UTF-8"))?;
    Ok(req)
}

/// Compare in a time that does not depend on where the texts differ.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |d, (x, y)| d | (x ^ y)) == 0
}

fn authorize(req: &Request, port: u16, token: &str) -> Result<(), Error> {
    let own = format!("127.0.0.1:{port}");
    if req.header("host") != Some(own.as_str()) {
        return Err(Error::new(
            403,
            "host.refused",
            format!("the Host must be {own}"),
        ));
    }
    if let Some(origin) = req.header("origin") {
        if origin != format!("http://{own}") {
            return Err(Error::new(
                403,
                "origin.refused",
                format!("Origin {origin} is not the daemon page"),
            ));
        }
    }
    let given = match req.header("authorization") {
        Some(v) => v.strip_prefix("Bearer "),
        None if req.is_page() => req.query_value("token"),
        None if req.socket_terminal().is_some() => req
            .words("sec-websocket-protocol")
            .find_map(|w| w.strip_prefix(TOKEN_PROTOCOL)),
        None => None,
    };
    let Some(given) = given else {
        return Err(Error::new(
            401,
            "token.missing",
            "send Authorization: Bearer <token>",
        ));
    };
    if !same_token(given, token) {
        return Err(Error::new(401, "token.invalid", "the token is not correct"));
    }
    Ok(())
}

/// A file of the wasm viewer. A daemon with no viewer folder, or a folder with no such
/// file, gives `viewer.missing`.
fn viewer_file(viewer: Option<&Path>, name: &str) -> Result<Reply, Error> {
    let Some(&(file, kind)) = VIEWER_FILES.iter().find(|(f, _)| *f == name) else {
        return Err(Error::new(
            404,
            "route.unknown",
            format!("no viewer file {name}"),
        ));
    };
    let missing = |why: String| {
        Error::new(
            404,
            "viewer.missing",
            format!("{why}. Build it with scripts/build-viewer.sh, or set FH_VIEWER"),
        )
    };
    let dir = viewer.ok_or_else(|| missing("the wasm viewer is not built".into()))?;
    let bytes = std::fs::read(dir.join(file))
        .map_err(|e| missing(format!("{}: {e}", dir.join(file).display())))?;
    Ok(Reply::Bytes(kind, bytes))
}

fn upgrade(ctx: &Ctx, req: &Request, id: &str) -> Result<Reply, Error> {
    let bad = |why: &str| Err(Error::bad("ws.invalid", why));
    let has = |name: &str, word: &str| req.words(name).any(|w| w.eq_ignore_ascii_case(word));
    if !has("upgrade", "websocket") || !has("connection", "upgrade") {
        return bad("send a WebSocket upgrade (Upgrade: websocket, Connection: Upgrade)");
    }
    if req.header("sec-websocket-version") != Some("13") {
        return bad("send Sec-WebSocket-Version: 13");
    }
    let Some(key) = req.header("sec-websocket-key").filter(|k| ws::valid_key(k)) else {
        return bad("send a Sec-WebSocket-Key of 16 bytes in base64");
    };
    if !req.words("sec-websocket-protocol").any(|w| w == PROTOCOL) {
        return bad("ask for the subprotocol fh.term.v1");
    }
    let t = ctx.sidebar.terminals.get(id)?;
    if ctx.sockets.fetch_add(1, Ordering::SeqCst) >= MAX_SOCKETS {
        ctx.sockets.fetch_sub(1, Ordering::SeqCst);
        return Err(Error::new(
            503,
            "term.busy",
            format!("{MAX_SOCKETS} terminal connections are open"),
        ));
    }
    Ok(Reply::Upgrade(t, ws::accept_key(key)))
}

fn switch(stream: &mut TcpStream, accept: &str) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\nSec-WebSocket-Protocol: {PROTOCOL}\r\n\r\n"
    )?;
    stream.flush()
}

fn terminal_record(m: &mut Message, t: &Terminal) {
    let info = t.info();
    m.set("terminal", &info.id);
    m.set("kind", info.kind.name());
    m.set("state", info.state);
    m.set("title", &info.title);
    if let Some(code) = info.exit {
        m.set("exit", code);
    }
}

fn pane_record(m: &mut Message, p: &Pane) {
    m.set("pane", &p.id);
    m.set("state", &p.state);
    m.set("terminal", p.terminal.as_deref().unwrap_or("-"));
    m.set("title", &p.title);
}

fn text_lines(req: &Request) -> Result<usize, Error> {
    Ok(match req.query_number("lines")? {
        0 => 50,
        n => (n as usize).min(HISTORY),
    })
}

fn route(ctx: &Ctx, req: &Request) -> Result<Reply, Error> {
    if req.is_page() {
        return Ok(Reply::Page(crate::random_hex(16)));
    }
    let (service, sidebar) = (&ctx.service, &ctx.sidebar);
    let segments: Vec<&str> = req.path.split('/').collect();
    let body = || Message::parse(&req.body);
    let mut m = Message::new();
    let pane = |p: &str| match valid_pane(p) {
        true => Ok(()),
        false => Err(Error::new(404, "pane.unknown", format!("no pane {p:?}"))),
    };
    match (req.method.as_str(), &segments[..]) {
        ("GET", ["", "viewer", name]) => return viewer_file(ctx.viewer.as_deref(), name),
        ("GET", ["", "api", "v1", "terminals"]) => {
            m.set("panes", sidebar.panes.status().kind);
            m.items = sidebar
                .terminals
                .list()
                .iter()
                .map(|t| t.info())
                .map(|i| format!("{} {} {} {}", i.id, i.kind.name(), i.state, i.title))
                .collect();
        }
        ("POST", ["", "api", "v1", "terminals"]) => {
            terminal_record(&mut m, &*sidebar.open(body()?.need("kind")?)?);
        }
        ("GET", ["", "api", "v1", "terminals", t]) => {
            terminal_record(&mut m, &*sidebar.terminals.get(t)?);
        }
        ("GET", ["", "api", "v1", "terminals", t, "text"]) => {
            let text = sidebar.terminals.get(t)?.text(text_lines(req)?);
            m.items = text.lines().map(str::to_owned).collect();
        }
        ("POST", ["", "api", "v1", "terminals", t, "close"]) => {
            let terminal = sidebar.terminals.get(t)?;
            sidebar.terminals.close(t)?;
            terminal_record(&mut m, &terminal);
        }
        ("GET", ["", "api", "v1", "panes"]) => {
            let status = sidebar.panes.status();
            m.set("backend", status.kind);
            m.set("detail", &status.detail);
            m.items = sidebar
                .panes
                .list()?
                .iter()
                .map(|p| {
                    let t = p.terminal.as_deref().unwrap_or("-");
                    format!("{} {} {t} {}", p.id, p.state, p.title)
                })
                .collect();
        }
        ("POST", ["", "api", "v1", "panes"]) => {
            let b = body()?;
            let spec = PaneSpec {
                title: b.need("title")?.to_owned(),
                cwd: b.get("cwd").map(PathBuf::from),
                command: b.get("command").map(str::to_owned),
            };
            pane_record(&mut m, &sidebar.panes.open(&spec)?);
        }
        ("GET", ["", "api", "v1", "panes", p]) => {
            pane(p)?;
            pane_record(&mut m, &sidebar.panes.get(p)?);
        }
        ("GET", ["", "api", "v1", "panes", p, "text"]) => {
            pane(p)?;
            let text = sidebar.panes.read(p, text_lines(req)?)?;
            m.items = text.lines().map(str::to_owned).collect();
        }
        ("POST", ["", "api", "v1", "panes", p, "send"]) => {
            pane(p)?;
            let b = body()?;
            let keys = b.get("keys").unwrap_or("").split_whitespace();
            let keys = keys.map(Key::parse).collect::<Result<Vec<_>, _>>()?;
            sidebar.panes.send(p, b.get("text").unwrap_or(""), &keys)?;
        }
        ("POST", ["", "api", "v1", "panes", p, "close"]) => {
            pane(p)?;
            sidebar.panes.close(p)?;
        }
        ("GET", ["", "api", "v1", "projects", p, "runs", r, "files", name]) => {
            return Ok(Reply::Bytes(
                "application/octet-stream",
                service.result_file(p, r, name)?,
            ));
        }
        ("GET", ["", "api", "v1", "projects"]) => {
            m.items = service
                .projects()
                .iter()
                .map(|(p, root)| format!("{p} {}", root.display()))
                .collect();
        }
        ("GET", ["", "api", "v1", "status"]) => {
            m.set("instance", service.instance());
            m.set("pid", std::process::id());
        }
        ("POST", ["", "api", "v1", "projects"]) => {
            let (project, root) = service.register(Path::new(body()?.need("root")?))?;
            m.set("project", project);
            m.set("root", root.display());
        }
        ("GET", ["", "api", "v1", "projects", p, "runs"]) => {
            m.items = service
                .runs(p)?
                .iter()
                .map(|r| format!("{} {}", r.run, r.state))
                .collect();
        }
        ("POST", ["", "api", "v1", "projects", p, "runs"]) => {
            let b = body()?;
            m = service
                .submit(p, b.need("request")?, b.need("input")?)?
                .to_message();
        }
        ("GET", ["", "api", "v1", "projects", p, "runs", r]) => {
            m = service.record(p, r)?.to_message();
        }
        ("POST", ["", "api", "v1", "projects", p, "runs", r, "cancel"]) => {
            m = service.cancel(p, r, body()?.need("request")?)?.to_message();
        }
        ("GET", ["", "api", "v1", "projects", p, "runs", r, "events"]) => {
            let since = req.query_number("since")?;
            let wait = Duration::from_millis(req.query_number("wait_ms")?);
            m = service.events(p, r, since, wait)?.to_message();
        }
        _ => {
            return Err(Error::new(
                404,
                "route.unknown",
                format!("no route {} {}", req.method, req.path),
            ))
        }
    }
    Ok(Reply::Message(m))
}

fn respond(stream: &mut TcpStream, answer: Result<Reply, Error>, port: u16) -> io::Result<()> {
    let text = "text/plain; charset=utf-8";
    let api_policy = "default-src 'none'; frame-ancestors 'none'".to_owned();
    let (status, kind, body, policy) = match answer {
        Ok(Reply::Message(m)) => (200, text, m.to_text().into_bytes(), api_policy),
        Ok(Reply::Page(nonce)) => (
            200,
            "text/html; charset=utf-8",
            PAGE.replace("NONCE", &nonce).into_bytes(),
            page_policy(&nonce, port),
        ),
        Ok(Reply::Bytes(kind, bytes)) => (200, kind, bytes, api_policy),
        Ok(Reply::Upgrade(..)) => unreachable!("the connection thread switches the protocol"),
        Err(e) => {
            let mut m = Message::new();
            m.set("error", &e.code);
            m.set("message", &e.message);
            (e.status, text, m.to_text().into_bytes(), api_policy)
        }
    };
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
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

/// The content security policy of the page: its own script (by nonce), inline styles (egui
fn page_policy(nonce: &str, port: u16) -> String {
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}' blob: 'wasm-unsafe-eval'; \
         style-src 'unsafe-inline'; connect-src 'self' ws://127.0.0.1:{port}; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(headers: &[(&str, &str)]) -> Request {
        Request {
            method: "GET".into(),
            path: "/api/v1/status".into(),
            query: String::new(),
            headers: headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            body: String::new(),
        }
    }

    #[test]
    fn access_rules() {
        let host = ("host", "127.0.0.1:7000");
        let token = ("authorization", "Bearer secret");
        let code = |h: &[(&str, &str)]| authorize(&request(h), 7000, "secret").map_err(|e| e.code);
        assert_eq!(code(&[host, token]), Ok(()));
        assert_eq!(
            code(&[host, token, ("origin", "http://127.0.0.1:7000")]),
            Ok(())
        );
        let refused = Err("origin.refused".to_owned());
        assert_eq!(
            code(&[host, token, ("origin", "http://evil.example")]),
            refused
        );
        assert_eq!(code(&[host, token, ("origin", "null")]), refused);
        assert_eq!(
            code(&[host, token, ("origin", "http://localhost:7000")]),
            refused
        );
        assert_eq!(
            code(&[("host", "evil.example:7000"), token]),
            Err("host.refused".into())
        );
        assert_eq!(code(&[host]), Err("token.missing".into()));
        let wrong = ("authorization", "Bearer secreT");
        assert_eq!(code(&[host, wrong]), Err("token.invalid".into()));
    }

    #[test]
    fn only_the_page_takes_the_token_in_the_query() {
        let host = ("host", "127.0.0.1:7000");
        let code = |path: &str, query: &str| {
            let mut req = request(&[host]);
            (req.path, req.query) = (path.into(), query.into());
            authorize(&req, 7000, "secret").map_err(|e| e.code)
        };
        assert_eq!(code("/", "token=secret"), Ok(()));
        assert_eq!(code("/", "token=secreT"), Err("token.invalid".into()));
        assert_eq!(code("/", ""), Err("token.missing".into()));
        let api = Err("token.missing".to_owned());
        assert_eq!(code("/api/v1/status", "token=secret"), api);
    }
}
