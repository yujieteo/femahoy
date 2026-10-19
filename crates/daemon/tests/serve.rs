//! The server of `fh serve` on a real socket: the token, `Host`, `Origin` and loopback rules,
//! the page policy, and the read of the run and preview folders that `fh run` writes.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fh_daemon::{new_preview, new_run, Daemon, Options};

struct Lab {
    dir: PathBuf,
    daemon: Daemon,
}

impl Lab {
    fn new(name: &str) -> Lab {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fh-serve-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("project")).unwrap();
        let daemon = Daemon::start_with_options(&dir.join("state"), Options::default()).unwrap();
        Lab { dir, daemon }
    }

    fn state(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// One raw HTTP request, as a browser or a script sends it.
    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)]) -> Answer {
        let port = self.daemon.port();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut head = format!("{method} {path} HTTP/1.1\r\n");
        if !headers.iter().any(|(n, _)| *n == "Host") {
            head.push_str(&format!("Host: 127.0.0.1:{port}\r\n"));
        }
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Content-Length: 0\r\n\r\n");
        stream.write_all(head.as_bytes()).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let split = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8(bytes[..split].to_vec()).unwrap();
        Answer {
            status: head[9..12].parse().unwrap(),
            head,
            body: bytes[split + 4..].to_vec(),
        }
    }

    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Answer {
        self.send("GET", path, headers)
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.daemon.token())
    }

    fn own(&self) -> String {
        format!("http://127.0.0.1:{}", self.daemon.port())
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

struct Answer {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Answer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().find_map(|l| {
            let (n, v) = l.split_once(": ")?;
            n.eq_ignore_ascii_case(name).then_some(v)
        })
    }

    fn error(&self) -> Option<String> {
        self.text()
            .lines()
            .find_map(|l| l.strip_prefix("error=").map(str::to_owned))
    }
}

#[test]
fn every_route_needs_the_token_and_the_own_origin_and_host() {
    let lab = Lab::new("access");
    let bearer = lab.bearer();
    let own = lab.own();
    let page = format!("/?token={}", lab.daemon.token());
    let routes = [
        ("GET", "/"),
        ("GET", "/viewer/fh_viewer.js"),
        ("GET", "/api/v1/chat"),
        ("POST", "/api/v1/chat"),
        ("POST", "/api/v1/chat/stop"),
        ("GET", "/api/v1/runs"),
        ("GET", "/api/v1/runs/run-000001/input"),
        ("GET", "/api/v1/runs/run-000001/files/results.pvd"),
        ("GET", "/api/v1/previews/1"),
    ];
    for (method, path) in routes {
        let no_token = lab.send(method, path, &[]);
        let wrong = format!("Bearer {}", "0".repeat(lab.daemon.token().len()));
        let wrong_token = lab.send(method, path, &[("Authorization", &wrong)]);
        let evil = |origin| {
            lab.send(
                method,
                path,
                &[("Authorization", &bearer), ("Origin", origin)],
            )
        };
        let wrong_host = lab.send(
            method,
            path,
            &[("Authorization", &bearer), ("Host", "evil.example")],
        );
        let own_origin = lab.send(
            method,
            path,
            &[("Authorization", &bearer), ("Origin", &own)],
        );
        assert_eq!(no_token.error().as_deref(), Some("token.missing"), "{path}");
        assert_eq!(
            wrong_token.error().as_deref(),
            Some("token.invalid"),
            "{path}"
        );
        assert_eq!(
            wrong_host.error().as_deref(),
            Some("host.refused"),
            "{path}"
        );
        for origin in [
            "http://evil.example",
            "null",
            &format!("http://localhost:{}", lab.daemon.port()),
            &format!("https://127.0.0.1:{}", lab.daemon.port()),
        ] {
            let answer = evil(origin);
            assert_eq!(
                answer.error().as_deref(),
                Some("origin.refused"),
                "{path} {origin}"
            );
            assert_eq!(answer.status, 403, "{path} {origin}");
        }
        // The token and the own origin pass. The answer can be any other error.
        assert!(
            ![401, 403].contains(&own_origin.status),
            "{method} {path}: {} {}",
            own_origin.status,
            own_origin.text()
        );
    }
    // Only the page takes the token in the query. The API needs the header.
    let api = format!("/api/v1/runs?token={}", lab.daemon.token());
    assert_eq!(lab.get(&api, &[]).error().as_deref(), Some("token.missing"));
    assert_eq!(lab.get(&page, &[]).status, 200);
    assert_eq!(
        lab.get("/?token=0123", &[]).error().as_deref(),
        Some("token.invalid")
    );
}

#[test]
fn the_daemon_listens_on_loopback_only() {
    let lab = Lab::new("loopback");
    // The address of this machine on its network, if it has one. A UDP connect sends no packet.
    let outward = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|s| s.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).map(|()| s))
        .and_then(|s| s.local_addr())
        .ok()
        .map(|a| a.ip())
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    if let Some(ip) = outward {
        let addr = SocketAddr::new(ip, lab.daemon.port());
        let connected = TcpStream::connect_timeout(&addr, Duration::from_secs(2));
        assert!(connected.is_err(), "the daemon answers on {addr}");
    }
}

#[test]
fn the_page_has_a_new_nonce_and_no_token() {
    let lab = Lab::new("page");
    let path = format!("/?token={}", lab.daemon.token());
    let page = lab.get(&path, &[]);
    assert_eq!(page.status, 200);
    let text = page.text();
    assert!(
        !text.contains(lab.daemon.token()),
        "the page holds no token"
    );
    let policy = page.header("Content-Security-Policy").unwrap().to_owned();
    let nonce = policy
        .split("'nonce-")
        .nth(1)
        .and_then(|n| n.split('\'').next())
        .unwrap()
        .to_owned();
    assert_eq!(nonce.len(), 32);
    assert!(text.contains(&format!("nonce=\"{nonce}\"")));
    assert!(policy.contains("connect-src 'self'") && policy.contains("frame-ancestors 'none'"));
    let sources: Vec<&str> = policy.split([' ', ';']).collect();
    assert!(sources.contains(&"'wasm-unsafe-eval'") && !sources.contains(&"'unsafe-eval'"));
    let again = lab.get(&path, &[]);
    assert!(!again
        .header("Content-Security-Policy")
        .unwrap()
        .contains(&nonce));
}

/// `fh run` writes the folders and the daemon reads them: a run is listed when it has results,
/// a run and a preview give the model with the lines of its include files in place, and a
/// file name must be plain.
#[test]
fn the_page_reads_the_run_and_preview_folders() {
    let lab = Lab::new("folders");
    let state = lab.state();
    let project = lab.dir.join("project");
    let mesh = "** units: SI-mm\n*NODE\n1, 0, 0, 0\n";
    let rest = "*BOUNDARY\n1, 1, 3\n";
    fs::write(project.join("mesh.inp"), mesh).unwrap();
    let file = project.join("job.inp");
    fs::write(&file, format!("*INCLUDE, INPUT=mesh.inp\n{rest}")).unwrap();
    let flat = format!("{mesh}{rest}");
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];

    // A run with no `results.pvd` is not in the list.
    let unfinished = new_run(&state, &file).unwrap();
    let run = new_run(&state, &file).unwrap();
    let vtu = b"\x00\xff<VTKFile>\r\n".to_vec();
    fs::create_dir_all(&run.out).unwrap();
    fs::write(run.out.join("step-1.vtu"), &vtu).unwrap();
    fs::write(
        run.out.join("results.pvd"),
        "<DataSet file=\"step-1.vtu\"/>\n",
    )
    .unwrap();
    let list = lab.get("/api/v1/runs", &auth).text();
    assert!(
        list.lines()
            .any(|l| l == format!("{} {}", run.id, file.display())),
        "{list}"
    );
    assert!(!list.contains(&unfinished.id), "{list}");

    let base = format!("/api/v1/runs/{}/files", run.id);
    let pvd = lab.get(&format!("{base}/results.pvd"), &auth);
    assert!(pvd.text().contains("file=\"step-1.vtu\""));
    let step = lab.get(&format!("{base}/step-1.vtu"), &auth);
    assert_eq!((step.status, step.body), (200, vtu), "binary bytes pass");
    // A later edit of the files does not change the model of the run.
    fs::write(&file, "*NODE\n").unwrap();
    fs::write(project.join("mesh.inp"), "*NODE\n").unwrap();
    let input = lab.get(&format!("/api/v1/runs/{}/input", run.id), &auth);
    assert_eq!((input.status, input.text()), (200, flat.clone()));

    let missing = lab.get(&format!("{base}/step-2.vtu"), &auth);
    assert_eq!(missing.error().as_deref(), Some("file.missing"));
    for name in [".hidden", "..", "a%2Fb", "x%20y"] {
        let bad = lab.get(&format!("{base}/{name}"), &auth);
        assert_eq!(bad.error().as_deref(), Some("file.invalid"), "{name}");
    }
    for path in [
        "/api/v1/runs/run-999999/files/results.pvd",
        "/api/v1/runs/../input",
        "/api/v1/runs/run-1/input",
    ] {
        let error = lab.get(path, &auth).error();
        assert_eq!(error.as_deref(), Some("run.unknown"), "{path}");
    }

    // A preview has no run.
    fs::write(project.join("mesh.inp"), mesh).unwrap();
    fs::write(&file, format!("*INCLUDE, INPUT=mesh.inp\n{rest}")).unwrap();
    let preview = new_preview(&state, &file, "job").unwrap();
    let shown = lab.get(&format!("/api/v1/previews/{preview}"), &auth);
    assert_eq!((shown.status, shown.text()), (200, flat));
    for id in ["x", "..", "7"] {
        let answer = lab.get(&format!("/api/v1/previews/{id}"), &auth);
        assert_eq!(answer.error().as_deref(), Some("preview.unknown"), "{id}");
    }

    // An include path that leaves the folder is refused.
    fs::write(&file, "*INCLUDE, INPUT=../mesh.inp\n").unwrap();
    assert_eq!(new_run(&state, &file).err().unwrap().code, "input.invalid");
    assert_eq!(
        new_preview(&state, &file, "x").err().unwrap().code,
        "input.invalid"
    );
}
