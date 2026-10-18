//! The terminal sidebar and the pane backend (DESIGN.md section 4.3): the token and
//! `Origin` rules on each terminal and pane route and on the WebSocket, a shell over the
//! WebSocket, and the PTY panes.

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fh_daemon::screen::{self, input_message, resize_message};
use fh_daemon::{Client, Config, Daemon, Error, Message, Options};
use fh_solver_api::SubprocessSolver;

struct TestDir(PathBuf);

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> TestDir {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("fh-term-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    TestDir(d)
}

fn start(dir: &Path, options: Options) -> Result<Daemon, Error> {
    let shell = Some(vec!["/bin/sh".into()]);
    let mut tries = 0;
    loop {
        let config = Config {
            state_dir: dir.join("state"),
            solver: SubprocessSolver::new("/bin/false"),
        };
        let options = Options {
            shell: shell.clone(),
            ..options.clone()
        };
        match Daemon::start_with_options(config, 0, options) {
            Err(e) if e.message.contains("Text file busy") && tries < 40 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            other => return other,
        }
    }
}

fn call(c: &Client, route: &str, fields: &[(&str, &str)]) -> Result<Message, Error> {
    let (method, path) = route.split_once(' ').unwrap();
    let mut m = Message::new();
    fields.iter().for_each(|(k, v)| m.set(k, v));
    let body = (method == "POST").then_some(&m);
    c.send(method, path, body, Duration::ZERO)
}

fn code<T>(r: Result<T, Error>) -> String {
    r.map_or_else(|e| e.code, |_| "ok".into())
}

fn wait_text(c: &Client, path: &str, want: &str) -> String {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let text = call(c, &format!("GET {path}"), &[])
            .unwrap()
            .items
            .join("\n");
        if text.contains(want) {
            return text;
        }
        assert!(Instant::now() < end, "no {want:?} in {text:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn raw(port: u16, method: &str, path: &str, headers: &[&str]) -> (u16, String) {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let head: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
    let host = format!("Host: 127.0.0.1:{port}");
    let request = format!("{method} {path} HTTP/1.1\r\n{host}\r\n{head}Content-Length: 0\r\n\r\n");
    s.write_all(request.as_bytes()).unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    let body = Message::parse(text.split_once("\r\n\r\n").unwrap().1).unwrap();
    let error = body.get("error").unwrap_or("").to_owned();
    (text[9..12].parse().unwrap(), error)
}

const ROUTES: &str = "GET /api/v1/terminals POST /api/v1/terminals GET /api/v1/terminals/t1 \
    GET /api/v1/terminals/t1/text POST /api/v1/terminals/t1/close GET /api/v1/terminals/t1/ws \
    GET /api/v1/panes POST /api/v1/panes GET /api/v1/panes/t1 GET /api/v1/panes/t1/text \
    POST /api/v1/panes/t1/send POST /api/v1/panes/t1/close";

const UPGRADE: [&str; 4] = [
    "Upgrade: websocket",
    "Connection: Upgrade",
    "Sec-WebSocket-Version: 13",
    "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==",
];

#[test]
fn every_terminal_and_pane_route_keeps_the_access_rules() {
    let d = dir("access");
    let daemon = start(&d, Options::default()).unwrap();
    let (port, token) = (daemon.port(), daemon.token().to_owned());
    let wrong = format!("Authorization: Bearer {}", "0".repeat(64));
    let right = format!("Authorization: Bearer {token}");
    let in_protocol = format!("Sec-WebSocket-Protocol: fh.term.v1, fh.token.{token}");
    let words: Vec<&str> = ROUTES.split_whitespace().collect();
    for route in words.chunks(2) {
        let (method, path) = (route[0], route[1]);
        let query = format!("{path}?token={token}");
        let carried = match path.ends_with("/ws") {
            true => (404, "term.unknown"),
            false => (401, "token.missing"),
        };
        let r = right.as_str();
        let cases = [
            (path, vec![], (401, "token.missing")),
            (path, vec![wrong.as_str()], (401, "token.invalid")),
            (
                path,
                vec![r, "Origin: http://evil.example"],
                (403, "origin.refused"),
            ),
            (
                path,
                vec![r, "Origin: http://localhost"],
                (403, "origin.refused"),
            ),
            (path, vec![r, "Origin: null"], (403, "origin.refused")),
            (&query, vec![], (401, "token.missing")),
            (path, vec![in_protocol.as_str()], carried),
        ];
        for (target, extra, (status, error)) in cases {
            let headers: Vec<&str> = UPGRADE.iter().copied().chain(extra).collect();
            let got = raw(port, method, target, &headers);
            assert_eq!(got, (status, error.into()), "{method} {target} {headers:?}");
        }
    }
    let list = call(&daemon.client(), "GET /api/v1/terminals", &[]).unwrap();
    assert!(list.items.is_empty());
}

#[test]
fn a_shell_runs_over_the_websocket_and_outlives_its_browser() {
    let d = dir("shell");
    let daemon = start(&d, Options::default()).unwrap();
    let c = daemon.client();
    let t = call(&c, "POST /api/v1/terminals", &[("kind", "shell")]).unwrap();
    let id = t.need("terminal").unwrap().to_owned();
    let got = (t.get("kind"), t.get("state"));
    assert_eq!(got, (Some("shell"), Some("running")));
    let page = c
        .clone()
        .with_origin(&format!("http://127.0.0.1:{}", c.port()));
    let mut ws = page.attach(&id, true).unwrap();
    let wrong = Client::new(c.port(), "0".repeat(64)).attach(&id, true);
    assert_eq!(code(wrong), "token.invalid");
    let evil = c.clone().with_origin("http://evil.example");
    assert_eq!(code(evil.attach(&id, false)), "origin.refused");
    assert_eq!(code(c.attach("t99", false)), "term.unknown");
    ws.send(&resize_message(60, 10)).unwrap();
    ws.send(&input_message(b"echo fh-$((6*7))\r")).unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    let found = |m: &[u8], s: &[u8]| m.windows(s.len()).any(|w| w == s);
    let m = loop {
        let m = ws.recv().unwrap();
        if m[0] == screen::MSG_SCREEN && found(&m, b"fh-42") {
            break m;
        }
        assert!(Instant::now() < end, "no output on the WebSocket");
    };
    assert_eq!(&m[1..5], &[0, 60, 0, 10]);
    wait_text(&c, &format!("/api/v1/terminals/{id}/text"), "fh-42");
    drop(ws);
    std::thread::sleep(Duration::from_millis(300));
    let route = format!("GET /api/v1/terminals/{id}");
    assert_eq!(call(&c, &route, &[]).unwrap().get("state"), Some("running"));
    let mut ws = c.attach(&id, false).unwrap();
    let first = ws.recv().unwrap();
    assert!(first[0] == screen::MSG_SCREEN && found(&first, b"fh-42"));
    ws.send(&input_message(b"exit 7\r")).unwrap();
    let exit = loop {
        let m = ws.recv().unwrap();
        if m[0] == screen::MSG_EXIT {
            break m;
        }
    };
    assert_eq!(exit, screen::exit_message(7));
    assert_eq!(ws.recv().unwrap_err().message, "1000");
    let t = call(&c, &route, &[]).unwrap();
    assert_eq!((t.get("state"), t.get("exit")), (Some("exited"), Some("7")));
    call(&c, &format!("POST /api/v1/terminals/{id}/close"), &[]).unwrap();
    let list = call(&c, "GET /api/v1/terminals", &[]).unwrap();
    assert!(list.items.is_empty());
    assert_eq!(list.get("panes"), Some("pty"));
}

#[test]
fn pty_panes_run_commands_and_take_keys() {
    let d = dir("pty");
    let daemon = start(&d, Options::default()).unwrap();
    let c = daemon.client();
    let status = call(&c, "GET /api/v1/panes", &[]).unwrap();
    assert_eq!(status.get("backend"), Some("pty"));
    let cwd = d.display().to_string();
    let command = "echo pane-$((2+3)); pwd";
    let fields = [("title", "build"), ("cwd", &cwd), ("command", command)];
    let p = call(&c, "POST /api/v1/panes", &fields).unwrap();
    let id = p.need("pane").unwrap().to_owned();
    assert_eq!(p.get("terminal"), Some(id.as_str()));
    let text = wait_text(&c, &format!("/api/v1/panes/{id}/text"), "pane-5");
    let folder = format!("fh-term-pty-{}", std::process::id());
    assert!(text.contains(&folder), "{text}");
    let send = format!("POST /api/v1/panes/{id}/send");
    let typed = [("text", "echo again-$((1+1))"), ("keys", "enter")];
    call(&c, &send, &typed).unwrap();
    wait_text(&c, &format!("/api/v1/panes/{id}/text?lines=2"), "again-2");
    let list = call(&c, "GET /api/v1/panes", &[]).unwrap().items;
    assert_eq!(list, [format!("{id} running {id} build")]);
    let bad = call(&c, &send, &[("keys", "enter bogus")]).unwrap_err();
    assert_eq!((bad.status, bad.code.as_str()), (400, "pane.key"));
    for (fields, error) in [
        (&[("title", "")][..], "pane.title"),
        (&[("title", "--clear")], "pane.title"),
        (&[("title", "x"), ("command", "-rf")], "pane.command"),
        (&[("title", "x"), ("cwd", "relative")], "pane.cwd"),
        (&[("cwd", "/")], "field.missing"),
    ] {
        assert_eq!(code(call(&c, "POST /api/v1/panes", fields)), error);
    }
    call(&c, &format!("POST /api/v1/panes/{id}/close"), &[]).unwrap();
    let gone = call(&c, &format!("GET /api/v1/panes/{id}"), &[]);
    assert_eq!(code(gone), "pane.unknown");
    let t = call(&c, "POST /api/v1/terminals", &[("kind", "shell")]).unwrap();
    let shell = format!("GET /api/v1/panes/{}", t.need("terminal").unwrap());
    assert_eq!(code(call(&c, &shell, &[])), "pane.unknown");
    let zsh = call(&c, "POST /api/v1/terminals", &[("kind", "zsh")]);
    assert_eq!(code(zsh), "term.kind");
}
