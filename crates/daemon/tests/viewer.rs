//! The routes of the wasm viewer: the page, the viewer files, the project list, the result
//! files of a run and the input of a revision. Each one keeps the token and `Origin` rules.

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fh_daemon::{new_request_id, Config, Daemon, Options, RunState};
use fh_solver_api::SubprocessSolver;

/// A solver that writes a small run folder at once.
const SOLVER: &str = r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=1"
printf '<VTKFile type="Collection"><Collection units="SI-mm"><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>\n' > results.pvd
printf 'VTU\000BYTES' > step-1.vtu
printf 'schema: fh.run\nversion: 1.1.0\nstatus: ok\n' > summary.toon
"#;

struct Lab {
    dir: PathBuf,
    daemon: Daemon,
}

impl Lab {
    /// A daemon with the fake solver. `viewer` makes a viewer folder with two small files.
    fn new(name: &str, viewer: bool) -> Lab {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fh-viewer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("project")).unwrap();
        let solver = dir.join("solver.sh");
        fs::write(&solver, SOLVER).unwrap();
        fs::set_permissions(&solver, fs::Permissions::from_mode(0o755)).unwrap();
        let viewer_dir = viewer.then(|| {
            let v = dir.join("viewer");
            fs::create_dir_all(&v).unwrap();
            fs::write(v.join("fh_viewer.js"), "export function start() {}\n").unwrap();
            fs::write(v.join("fh_viewer_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
            v
        });
        let config = Config {
            state_dir: dir.join("state"),
            solver: SubprocessSolver::new(&solver),
            solver_dirs: Vec::new(),
        };
        let options = Options { viewer: viewer_dir };
        let daemon = Daemon::start_with_options(config, 0, options).unwrap();
        Lab { dir, daemon }
    }

    fn get(&self, path: &str, headers: &[(&str, &str)]) -> Answer {
        let port = self.daemon.port();
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut head = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
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

    fn bearer(&self) -> String {
        format!("Bearer {}", self.daemon.token())
    }

    /// Register the project, run the fake solver and wait until the run succeeds.
    fn run(&self) -> (String, String) {
        let c = self.daemon.client();
        let project = c.register(&self.dir.join("project")).unwrap();
        fs::write(self.dir.join("project/model.inp"), "*NODE\n").unwrap();
        let ack = c.submit(&project, &new_request_id(), "model.inp").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while c.record(&project, &ack.run).unwrap().state != RunState::Succeeded {
            assert!(Instant::now() < deadline, "the run does not succeed");
            std::thread::sleep(Duration::from_millis(20));
        }
        (project, ack.run)
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
fn the_page_loads_the_viewer_with_a_new_nonce_and_no_token_in_it() {
    let lab = Lab::new("page", true);
    let page = lab.get(&format!("/?token={}", lab.daemon.token()), &[]);
    assert_eq!(page.status, 200);
    assert_eq!(
        page.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    let text = page.text();
    assert!(text.contains("/viewer/fh_viewer.js") && text.contains("viewer.start("));
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
    // A second answer has a new nonce.
    let again = lab.get(&format!("/?token={}", lab.daemon.token()), &[]);
    assert!(!again
        .header("Content-Security-Policy")
        .unwrap()
        .contains(&nonce));
}

#[test]
fn the_page_has_the_chat_beside_the_model_view_and_loads_nothing_from_the_network() {
    let lab = Lab::new("layout", true);
    let text = lab
        .get(&format!("/?token={}", lab.daemon.token()), &[])
        .text();
    let at = |part: &str| {
        text.find(part)
            .unwrap_or_else(|| panic!("the page has no {part}"))
    };
    // The chat is on the left of the model view, and its composer is below its messages.
    assert!(at("<aside id=\"chat\"") < at("<main id=\"model\""));
    assert!(at("id=\"log\"") < at("<textarea") && at("<textarea") < at("id=\"send\""));
    assert!(at("<canvas id=\"fh\"") > at("<main id=\"model\""));
    for part in [
        "id=\"stop\"",
        "id=\"runs\"",
        "\"/api/v1/chat?since=\"",
        "\"/api/v1/chat/stop\"",
    ] {
        at(part);
    }
    // The font comes from the viewer. The page names no other host.
    assert!(text.contains("new FontFace(\"Inter\", viewer.font()"));
    assert!(text.contains("font: 15px/1.55 Inter,"));
    for remote in ["http:", "https:", "//cdn", "@import", "<link"] {
        assert!(!text.contains(remote), "the page has {remote}");
    }
}

#[test]
fn the_viewer_files_need_the_token_and_the_own_origin() {
    let lab = Lab::new("files", true);
    let bearer = lab.bearer();
    let js = lab.get("/viewer/fh_viewer.js", &[("Authorization", &bearer)]);
    assert_eq!(js.status, 200, "{}", js.text());
    assert_eq!(js.header("Content-Type"), Some("text/javascript"));
    let wasm = lab.get("/viewer/fh_viewer_bg.wasm", &[("Authorization", &bearer)]);
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.header("Content-Type"), Some("application/wasm"));
    assert_eq!(wasm.body, b"\0asm\x01\0\0\0");
    // The token in the query is only for the page.
    let query = format!("/viewer/fh_viewer.js?token={}", lab.daemon.token());
    assert_eq!(
        lab.get(&query, &[]).error().as_deref(),
        Some("token.missing")
    );
    let evil = [
        ("Authorization", bearer.as_str()),
        ("Origin", "http://evil.example"),
    ];
    assert_eq!(lab.get("/viewer/fh_viewer.js", &evil).status, 403);
    let other = lab.get("/viewer/secret.txt", &[("Authorization", &bearer)]);
    assert_eq!(other.error().as_deref(), Some("route.unknown"));
}

#[test]
fn a_daemon_with_no_viewer_says_how_to_build_it() {
    let lab = Lab::new("no-viewer", false);
    let js = lab.get("/viewer/fh_viewer.js", &[("Authorization", &lab.bearer())]);
    assert_eq!(js.status, 404);
    assert_eq!(js.error().as_deref(), Some("viewer.missing"));
    assert!(
        js.text().contains("scripts/build-viewer.sh"),
        "{}",
        js.text()
    );
}

#[test]
fn the_projects_and_the_result_files_of_a_run() {
    let lab = Lab::new("results", true);
    let (project, run) = lab.run();
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];
    let list = lab.get("/api/v1/projects", &auth).text();
    let root = fs::canonicalize(lab.dir.join("project")).unwrap();
    assert!(
        list.lines()
            .any(|l| l == format!("{project} {}", root.display())),
        "{list}"
    );
    let base = format!("/api/v1/projects/{project}/runs/{run}/files");
    let pvd = lab.get(&format!("{base}/results.pvd"), &auth);
    assert_eq!(pvd.status, 200);
    assert!(pvd.text().contains("file=\"step-1.vtu\""));
    let vtu = lab.get(&format!("{base}/step-1.vtu"), &auth);
    assert_eq!(vtu.body, b"VTU\0BYTES", "binary bytes pass unchanged");
    assert_eq!(vtu.header("Content-Type"), Some("application/octet-stream"));

    let missing = lab.get(&format!("{base}/step-2.vtu"), &auth);
    assert_eq!(
        (missing.status, missing.error().as_deref()),
        (404, Some("file.missing"))
    );
    for name in [".hidden", "..", "a%2Fb", "x%20y"] {
        let bad = lab.get(&format!("{base}/{name}"), &auth);
        assert_eq!(bad.error().as_deref(), Some("file.invalid"), "{name}");
    }
    let path = format!("/api/v1/projects/{project}/runs/run-999999/files/results.pvd");
    assert_eq!(
        lab.get(&path, &auth).error().as_deref(),
        Some("run.unknown")
    );
    let path = format!("/api/v1/projects/p-0000000000000000/runs/{run}/files/results.pvd");
    assert_eq!(
        lab.get(&path, &auth).error().as_deref(),
        Some("project.unknown")
    );

    // The token and the Origin rules hold for the result files.
    let file = format!("{base}/results.pvd");
    assert_eq!(
        lab.get(&file, &[]).error().as_deref(),
        Some("token.missing")
    );
    let evil = [("Authorization", bearer.as_str()), ("Origin", "null")];
    assert_eq!(
        lab.get(&file, &evil).error().as_deref(),
        Some("origin.refused")
    );
    let own = format!("http://127.0.0.1:{}", lab.daemon.port());
    let page = [("Authorization", bearer.as_str()), ("Origin", own.as_str())];
    assert_eq!(lab.get(&file, &page).status, 200);
}

#[test]
fn the_model_before_the_solve_is_the_revision_that_the_run_solves() {
    let lab = Lab::new("preview", false);
    let c = lab.daemon.client();
    let project = c.register(&lab.dir.join("project")).unwrap();
    let input = "** units: SI-mm\n*NODE\n1, 0, 0, 0\n";
    fs::write(lab.dir.join("project/model.inp"), input).unwrap();
    let revision = c.preview(&project, "model.inp", "a case").unwrap();
    assert!(
        c.runs(&project).unwrap().is_empty(),
        "a preview solves nothing"
    );
    let path = format!("/api/v1/projects/{project}/revisions/{revision}");
    let bearer = lab.bearer();
    let own = format!("http://127.0.0.1:{}", lab.daemon.port());
    let page = [("Authorization", bearer.as_str()), ("Origin", own.as_str())];
    let answer = lab.get(&path, &page);
    assert_eq!(
        (answer.status, answer.body.as_slice()),
        (200, input.as_bytes())
    );
    let ack = c.submit(&project, &new_request_id(), "model.inp").unwrap();
    assert_eq!(c.record(&project, &ack.run).unwrap().revision, revision);

    // The token and the Origin rules hold, and only a revision of the project is read.
    assert_eq!(
        lab.get(&path, &[]).error().as_deref(),
        Some("token.missing")
    );
    let evil = [("Authorization", bearer.as_str()), ("Origin", "null")];
    assert_eq!(
        lab.get(&path, &evil).error().as_deref(),
        Some("origin.refused")
    );
    let auth = [("Authorization", bearer.as_str())];
    let base = format!("/api/v1/projects/{project}/revisions");
    for (rev, code) in [
        ("rev-x", "revision.invalid"),
        ("..", "revision.invalid"),
        ("rev-0000000000000007", "revision.unknown"),
    ] {
        let answer = lab.get(&format!("{base}/{rev}"), &auth);
        assert_eq!(answer.error().as_deref(), Some(code), "{rev}");
    }
    let other = format!("/api/v1/projects/p-0000000000000000/revisions/{revision}");
    assert_eq!(
        lab.get(&other, &auth).error().as_deref(),
        Some("project.unknown")
    );
    fs::write(lab.dir.join("outside.inp"), input).unwrap();
    let outside = c.preview(&project, "../outside.inp", "x").unwrap_err();
    assert_eq!(outside.code, "input.outside");
}
