//! The server of `fh serve` on a real socket: the token, `Host`, `Origin` and loopback rules,
//! the page policy, and the read of the run and preview folders that `fh run` writes.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use fh::daemon::{new_preview, new_run, Daemon, Options};

mod common;
use common::{ccx_solver, p, root};

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

    /// Check that the request failed with `status` and a `message` field that holds `text`.
    fn assert_failed(&self, status: u16, text: &str, what: &str) {
        let body = self.text();
        let message = body.lines().find_map(|l| l.strip_prefix("message="));
        assert_eq!(self.status, status, "{what}: {body}");
        assert!(message.is_some_and(|m| m.contains(text)), "{what}: {body}");
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
        no_token.assert_failed(401, "send Authorization", path);
        wrong_token.assert_failed(401, "the token is not correct", path);
        wrong_host.assert_failed(403, "the Host must be", path);
        for origin in [
            "http://evil.example",
            "null",
            &format!("http://localhost:{}", lab.daemon.port()),
            &format!("https://127.0.0.1:{}", lab.daemon.port()),
        ] {
            let answer = evil(origin);
            answer.assert_failed(403, "is not the daemon page", &format!("{path} {origin}"));
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
    lab.get(&api, &[])
        .assert_failed(401, "send Authorization", &api);
    assert_eq!(lab.get(&page, &[]).status, 200);
    lab.get("/?token=0123", &[])
        .assert_failed(401, "the token is not correct", "/?token=0123");
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
    missing.assert_failed(404, "has no file step-2.vtu", "step-2");
    for name in [".hidden", "..", "a%2Fb", "x%20y"] {
        let bad = lab.get(&format!("{base}/{name}"), &auth);
        bad.assert_failed(400, "is not a plain file name", name);
    }
    for path in [
        "/api/v1/runs/run-999999/files/results.pvd",
        "/api/v1/runs/../input",
        "/api/v1/runs/run-1/input",
    ] {
        lab.get(path, &auth).assert_failed(404, "no run", path);
    }

    // A preview has no run.
    fs::write(project.join("mesh.inp"), mesh).unwrap();
    fs::write(&file, format!("*INCLUDE, INPUT=mesh.inp\n{rest}")).unwrap();
    let preview = new_preview(&state, &file, "job").unwrap();
    let shown = lab.get(&format!("/api/v1/previews/{preview}"), &auth);
    assert_eq!((shown.status, shown.text()), (200, flat));
    for id in ["x", "..", "7"] {
        let answer = lab.get(&format!("/api/v1/previews/{id}"), &auth);
        answer.assert_failed(404, "no preview", id);
    }

    // An include path that leaves the folder is refused.
    fs::write(&file, "*INCLUDE, INPUT=../mesh.inp\n").unwrap();
    let run = new_run(&state, &file).err().unwrap();
    assert!(run.contains("write a relative path"), "{run}");
    let preview = new_preview(&state, &file, "x").unwrap_err();
    assert!(preview.contains("write a relative path"), "{preview}");
}

/// A topology case keeps the `*INCLUDE` files of its `job.inp` that are in a subfolder.
#[test]
fn a_topology_run_copies_the_include_files_of_its_job() {
    let lab = Lab::new("topology-include");
    let case = lab.dir.join("project");
    let mesh = "*NODE\n1, 0, 0, 0\n";
    let rest = "*BOUNDARY\n1, 1, 3\n";
    fs::create_dir_all(case.join("mesh")).unwrap();
    fs::write(case.join("mesh/part.inp"), mesh).unwrap();
    fs::write(
        case.join("job.inp"),
        format!("*INCLUDE, INPUT=mesh/part.inp\n{rest}"),
    )
    .unwrap();
    fs::write(case.join("topology.toml"), "volume = 0.5\nradius = 1.5\n").unwrap();

    let run = new_run(&lab.state(), &case).unwrap();
    assert!(run.input.join("topology.toml").is_file());
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];
    let input = lab.get(&format!("/api/v1/runs/{}/input", run.id), &auth);
    assert_eq!((input.status, input.text()), (200, format!("{mesh}{rest}")));
}

/// The solve of a case that asks for no field (no `*NODE FILE`, no `*EL FILE`) ends with code 0
/// and a `results.pvd` with no step file. The viewer shows such a run as an error, so the list
/// leaves it out.
#[test]
fn a_run_that_wrote_no_field_is_not_listed() {
    let Some(solver) = ccx_solver() else { return };
    let lab = Lab::new("no-field");
    let input = root().join("validation/cases/bar-pressure/job.inp");
    let out = Command::new(env!("CARGO_BIN_EXE_fh"))
        .args(["run", p(&input)])
        .env("FH_STATE", lab.state())
        .env("FH_CCX", &solver)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let pvd = fs::read_to_string(lab.state().join("runs/run-000001/out/results.pvd")).unwrap();
    assert!(!pvd.contains("<DataSet"), "the run wrote a field: {pvd}");
    let bearer = lab.bearer();
    let list = lab
        .get("/api/v1/runs", &[("Authorization", &bearer)])
        .text();
    assert_eq!(list.trim(), "", "the list holds the run: {list}");
}

/// Two daemons use one state folder. The start of the second daemon keeps the preview that the
/// folder holds, and the chat of the second daemon does not show it as new.
#[test]
fn two_daemons_use_one_state_folder() {
    let first = Lab::new("two");
    let state = first.state();
    let file = first.dir.join("project/job.inp");
    fs::write(&file, "*NODE\n").unwrap();
    let preview = new_preview(&state, &file, "job").unwrap();

    let daemon = Daemon::start_with_options(&state, Options::default());
    let second = Lab {
        dir: first.dir.clone(),
        daemon: daemon.expect("a second daemon starts"),
    };
    for lab in [&first, &second] {
        let bearer = lab.bearer();
        let auth = [("Authorization", bearer.as_str())];
        let shown = lab.get(&format!("/api/v1/previews/{preview}"), &auth);
        assert_eq!(shown.status, 200, "{}", shown.text());
    }
    // The second daemon started after the preview, so its chat does not show it as new.
    let bearer = second.bearer();
    let chat = second.get("/api/v1/chat?since=0", &[("Authorization", &bearer)]);
    assert!(!chat.text().contains("show"), "{}", chat.text());
}

/// A browser opens spare connections and can send its next request on one of them. A connection
/// that sends no byte before the timeout gets no answer, because a 400 on it can be the
/// answer to that next request. A request that is really bad still gets a 400.
#[test]
fn a_connection_that_sends_nothing_gets_no_answer() {
    let lab = Lab::new("silent");
    let port = lab.daemon.port();
    let mut idle = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    idle.set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut bytes = Vec::new();
    idle.read_to_end(&mut bytes).unwrap();
    assert!(bytes.is_empty(), "{}", String::from_utf8_lossy(&bytes));

    let mut bad = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    bad.write_all(b"BAD\r\n\r\n").unwrap();
    let mut bytes = Vec::new();
    bad.read_to_end(&mut bytes).unwrap();
    let head = String::from_utf8_lossy(&bytes);
    assert!(head.starts_with("HTTP/1.1 400 "), "{head}");
}

/// Artifacts stay inside the retained run and the report has an opaque sandbox origin.
#[test]
fn retained_artifacts_need_authorization_and_reports_cannot_escape_the_run() {
    let lab = Lab::new("artifacts");
    let run = lab.state().join("runs/run-000001");
    fs::create_dir_all(run.join("out")).unwrap();
    fs::create_dir_all(run.join("in/system")).unwrap();
    fs::write(run.join("in/system/controlDict"), "retained case input").unwrap();
    fs::write(run.join("out/step-1.vtu"), b"mesh").unwrap();
    fs::write(run.join("report.html"), b"<h1>Retained report</h1>").unwrap();
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];
    let base = "/api/v1/runs/run-000001";
    let list = lab.get(&format!("{base}/artifacts"), &auth);
    assert_eq!(list.status, 200);
    let items: Vec<_> = list.text().lines().map(str::to_owned).collect();
    assert_eq!(
        items,
        [
            "",
            "files/step-1.vtu",
            "inputs/0 system/controlDict",
            "report"
        ]
    );
    assert_eq!(
        lab.get(&format!("{base}/inputs/0"), &auth).text(),
        "retained case input"
    );
    assert_eq!(lab.get(&format!("{base}/inputs/1"), &auth).status, 404);
    assert_eq!(lab.get(&format!("{base}/report"), &[]).status, 401);
    let report = lab.get(&format!("{base}/report"), &auth);
    assert_eq!(report.text(), "<h1>Retained report</h1>");
    let policy = report.header("Content-Security-Policy").unwrap();
    assert!(policy.starts_with("sandbox allow-scripts;"));
    assert!(!policy.contains("allow-same-origin"));
    assert!(policy.contains("connect-src 'none'"));
    let token_url = format!("{base}/report?token={}", lab.daemon.token());
    assert_eq!(lab.get(&token_url, &[]).status, 401);
    let issued = lab.send("POST", &format!("{base}/report-ticket"), &auth);
    assert_eq!(issued.status, 200);
    let body = issued.text();
    let ticket = body
        .lines()
        .find_map(|l| l.strip_prefix("ticket="))
        .unwrap();
    assert!(!body.contains(lab.daemon.token()));
    let url = format!("{base}/report?ticket={ticket}");
    assert_eq!(lab.get(&format!("/?ticket={ticket}"), &[]).status, 401);
    assert_eq!(lab.get(&url, &[("Origin", "null")]).status, 403);
    assert_eq!(lab.get(&url, &[]).status, 200);
    assert_eq!(lab.get(&url, &[]).status, 401);
    fs::remove_file(run.join("report.html")).unwrap();
    assert_eq!(lab.get(&format!("{base}/report"), &auth).status, 404);
    #[cfg(unix)]
    {
        let outside = lab.dir.join("outside.html");
        fs::write(&outside, "private").unwrap();
        std::os::unix::fs::symlink(&outside, run.join("report.html")).unwrap();
        std::os::unix::fs::symlink(&outside, run.join("out/secret.vtu")).unwrap();
        assert_eq!(
            lab.get(&format!("{base}/files/secret.vtu"), &auth).status,
            403
        );
        assert_eq!(lab.get(&format!("{base}/report"), &auth).status, 403);
        fs::write(lab.dir.join("report.html"), "private outside run").unwrap();
        std::os::unix::fs::symlink(&lab.dir, lab.state().join("runs/run-000002")).unwrap();
        assert_eq!(lab.get("/api/v1/runs/run-000002/report", &auth).status, 403);
    }
}

/// The token is in the state folder with owner-only permissions, and a new start reads it, so
/// the address of the page stays the same. A token file that other users can read stops the start.
#[test]
fn the_token_stays_the_same_at_each_start() {
    use std::os::unix::fs::PermissionsExt;

    let lab = Lab::new("token");
    let file = lab.state().join("token");
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "mode {mode:o}");
    assert_eq!(fs::read_to_string(&file).unwrap(), lab.daemon.token());

    let again = Daemon::start_with_options(&lab.state(), Options::default()).unwrap();
    assert_eq!(again.token(), lab.daemon.token());

    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let err = Daemon::start_with_options(&lab.state(), Options::default())
        .err()
        .expect("a token that other users can read stops the start");
    assert!(err.contains("chmod 600"), "{err}");
}

/// A port in use stops the start with a message that names the port and `FH_PORT`. The daemon
/// does not use another port.
#[test]
fn a_port_in_use_stops_the_start() {
    let lab = Lab::new("port");
    let options = Options {
        port: lab.daemon.port(),
        ..Options::default()
    };
    let err = Daemon::start_with_options(&lab.state(), options)
        .err()
        .expect("a second daemon on the same port does not start");
    assert!(err.contains(&lab.daemon.port().to_string()), "{err}");
    assert!(err.contains("FH_PORT"), "{err}");
}
