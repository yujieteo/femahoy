//! `fh serve`: the daemon with its token, `Origin` and loopback rules, and its stop flag.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

fn state_dir(name: &str) -> PathBuf {
    let state = std::env::temp_dir().join(format!("fh-serve-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    state
}

/// Wait for `daemon.info` and read the port and the token.
fn wait_for_info(state: &Path) -> (u16, String) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let text = loop {
        if let Ok(text) = std::fs::read_to_string(state.join("daemon.info")) {
            break text;
        }
        assert!(Instant::now() < deadline, "no daemon.info");
        std::thread::sleep(Duration::from_millis(20));
    };
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .unwrap_or_else(|| panic!("no {key} in daemon.info"))
            .to_owned()
    };
    (field("port").parse().unwrap(), field("token"))
}

/// `fh serve` on a new state folder, in a thread, with its own stop flag.
struct Serve {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<ExitCode>>,
    state: PathBuf,
    port: u16,
    token: String,
}

impl Serve {
    fn start(name: &str) -> Serve {
        let state = state_dir(name);
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, dir) = (stop.clone(), state.clone());
        // A test never opens the browser of the machine.
        let thread = std::thread::spawn(move || fh_daemon::serve_until(&dir, &flag, false));
        let (port, token) = wait_for_info(&state);
        Serve {
            stop,
            thread: Some(thread),
            state,
            port,
            token,
        }
    }

    fn own(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn get(&self, path: &str, headers: &[(&str, &str)]) -> (u16, String) {
        request(self.port, "GET", path, headers, "")
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_dir_all(&self.state);
    }
}

/// One raw HTTP request, as a browser or a script sends it. The `Host` is the daemon's own.
fn request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
    stream.write_all(head.as_bytes()).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().expect("status code");
    let body = answer.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    (status, body.to_owned())
}

fn error_code(body: &str) -> Option<&str> {
    body.lines().find_map(|l| l.strip_prefix("error="))
}

#[test]
fn a_request_with_no_token_fails() {
    let serve = Serve::start("no-token");
    let (status, body) = serve.get("/api/v1/status", &[]);
    assert_eq!(status, 401, "{body}");
    assert_eq!(error_code(&body), Some("token.missing"));
    let (status, body) = serve.get("/", &[]);
    assert_eq!(status, 401, "the page also needs the token: {body}");
    assert_eq!(error_code(&body), Some("token.missing"));
}

#[test]
fn a_request_with_a_wrong_token_fails() {
    let serve = Serve::start("wrong-token");
    let wrong = format!("Bearer {}", "0".repeat(serve.token.len()));
    let (status, body) = serve.get("/api/v1/status", &[("Authorization", &wrong)]);
    assert_eq!(status, 401, "{body}");
    assert_eq!(error_code(&body), Some("token.invalid"));
    let (status, body) = serve.get("/?token=0123", &[]);
    assert_eq!(status, 401, "{body}");
    assert_eq!(error_code(&body), Some("token.invalid"));
    // The token in the query is only for the page. The API needs the header.
    let query = format!("/api/v1/status?token={}", serve.token);
    let (status, body) = serve.get(&query, &[]);
    assert_eq!(status, 401, "{body}");
    assert_eq!(error_code(&body), Some("token.missing"));
}

/// The answer to a POST that tells a body of `length` bytes and sends none. The daemon
/// refuses a body that is too large from the head alone, so it never reads the body.
fn head_only(serve: &Serve, path: &str, length: usize) -> (u16, String) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, serve.port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: {}\r\n\
         Content-Length: {length}\r\n\r\n",
        serve.port,
        serve.bearer()
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    let body = answer.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    (answer[9..12].parse().unwrap(), body.to_owned())
}

/// A chat message can hold a long pasted paper, and the chat route keeps a limit. Every other
/// route keeps the small limit of 16 KiB.
#[test]
fn a_chat_message_can_hold_a_long_paper_and_the_limit_stays() {
    let serve = Serve::start("body-limit");
    let auth = serve.bearer();
    let headers = [("Authorization", auth.as_str())];
    let code = |body: &str| error_code(body).map(str::to_owned);
    // 200000 characters: more than a long paper. No agent runs in this test, so the answer
    // can be any error, but not a refusal of the size.
    let paper = format!("text={}\n", "a ".repeat(100_000));
    let (status, body) = request(serve.port, "POST", "/api/v1/chat", &headers, &paper);
    assert_ne!(
        code(&body).as_deref(),
        Some("http.too-large"),
        "{status} {body}"
    );
    let refused = |(status, body): (u16, String)| (status, code(&body));
    let too_large = (413, Some("http.too-large".to_owned()));
    assert_eq!(
        refused(head_only(&serve, "/api/v1/chat", 2 << 20)),
        too_large
    );
    assert_eq!(
        refused(head_only(&serve, "/api/v1/runs", 17 << 10)),
        too_large
    );
}

#[test]
fn a_request_from_a_wrong_origin_fails() {
    let serve = Serve::start("wrong-origin");
    let bearer = serve.bearer();
    let page = format!("/?token={}", serve.token);
    for origin in [
        "http://evil.example",
        "null",
        &format!("http://localhost:{}", serve.port),
        &format!("https://127.0.0.1:{}", serve.port),
    ] {
        let (status, body) = serve.get(
            "/api/v1/status",
            &[("Authorization", &bearer), ("Origin", origin)],
        );
        assert_eq!(status, 403, "{origin}: {body}");
        assert_eq!(error_code(&body), Some("origin.refused"), "{origin}");
        let (status, body) = serve.get(&page, &[("Origin", origin)]);
        assert_eq!(status, 403, "{origin}: {body}");
    }
    let input = serve.state.join("model.inp");
    std::fs::write(&input, "*NODE\n").unwrap();
    let body = format!("input={}\n\n", input.display());
    let (status, answer) = request(
        serve.port,
        "POST",
        "/api/v1/runs",
        &[
            ("Authorization", &bearer),
            ("Origin", "http://evil.example"),
        ],
        &body,
    );
    assert_eq!(status, 403, "{answer}");
    assert_eq!(error_code(&answer), Some("origin.refused"));
}

#[test]
fn the_token_and_the_own_origin_pass() {
    let serve = Serve::start("own-origin");
    let bearer = serve.bearer();
    let (status, body) = serve.get("/api/v1/status", &[("Authorization", &bearer)]);
    assert_eq!(status, 200, "{body}");
    let own = serve.own();
    let (status, body) = serve.get(
        "/api/v1/status",
        &[("Authorization", &bearer), ("Origin", &own)],
    );
    assert_eq!(status, 200, "{body}");
    let (status, page) = serve.get(&format!("/?token={}", serve.token), &[]);
    assert_eq!(status, 200, "{page}");
    assert!(page.starts_with("<!doctype html>"), "{page}");
    let (status, page) = serve.get("/", &[("Authorization", &bearer)]);
    assert_eq!(status, 200, "{page}");
}

#[test]
fn the_daemon_listens_on_loopback_only() {
    let serve = Serve::start("loopback");
    // The address of this machine on its network, if it has one. A UDP connect sends no packet.
    let outward = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|s| s.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).map(|()| s))
        .and_then(|s| s.local_addr())
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    if let Some(ip) = outward {
        let addr = SocketAddr::new(ip, serve.port);
        let connected = TcpStream::connect_timeout(&addr, Duration::from_secs(2));
        assert!(connected.is_err(), "the daemon answers on {addr}");
    }
    let (status, _) = serve.get("/api/v1/status", &[("Authorization", &serve.bearer())]);
    assert_eq!(status, 200);
}

#[test]
fn a_second_daemon_on_the_same_state_folder_fails() {
    let serve = Serve::start("second");
    let never = AtomicBool::new(false);
    let second = fh_daemon::serve_until(&serve.state, &never, false);
    assert_eq!(second, ExitCode::from(1));
}

#[test]
fn the_stop_flag_stops_the_daemon_and_removes_daemon_info() {
    let mut serve = Serve::start("stop");
    serve.stop.store(true, Ordering::SeqCst);
    let outcome = serve.thread.take().unwrap().join().unwrap();
    assert_eq!(outcome, ExitCode::SUCCESS);
    assert!(!serve.state.join("daemon.info").exists());
}
#[test]
fn serve_refuses_a_bad_option() {
    let err = fh_daemon::serve(&["--host".to_owned(), "0.0.0.0".to_owned()]).unwrap_err();
    assert!(err.contains("unknown option: --host"), "{err}");
}
