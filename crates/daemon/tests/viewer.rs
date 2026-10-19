//! The routes of the wasm viewer: the page, the viewer files, the run list, the result files
//! and the input of a run, and the input of a preview. Each one keeps the token and `Origin`
//! rules.

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fh_daemon::{Config, Daemon, Limits, Options, RunState};
use fh_solver_ccx::Ccx;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/ccx")
        .join(name)
}

struct Lab {
    dir: PathBuf,
    daemon: Daemon,
}

impl Lab {
    /// A daemon with the fake `ccx`. `viewer` makes a viewer folder with two small files.
    fn new(name: &str, viewer: bool) -> Lab {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("fh-viewer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("project")).unwrap();
        let viewer_dir = viewer.then(|| {
            let v = dir.join("viewer");
            fs::create_dir_all(&v).unwrap();
            fs::write(v.join("fh_viewer.js"), "export function start() {}\n").unwrap();
            fs::write(v.join("fh_viewer_bg.wasm"), b"\0asm\x01\0\0\0").unwrap();
            v
        });
        let config = Config {
            state_dir: dir.join("state"),
            solver: Ccx {
                exe: fixture("fake-ccx.sh"),
                ..Ccx::new()
            },
        };
        let options = Options { viewer: viewer_dir };
        let daemon = Daemon::start_with_options(config, options).unwrap();
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

    /// Run the fake `ccx` on `project/model.inp` and wait until the run succeeds.
    fn run(&self) -> String {
        let c = self.daemon.client();
        let input = self.dir.join("project/model.inp");
        fs::copy(fixture("tet.inp"), &input).unwrap();
        let run = c.submit(&input, &Limits::default()).unwrap().run;
        let deadline = Instant::now() + Duration::from_secs(30);
        while c.record(&run).unwrap().state != RunState::Succeeded {
            assert!(Instant::now() < deadline, "the run does not succeed");
            std::thread::sleep(Duration::from_millis(20));
        }
        run
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
fn the_runs_and_the_files_of_a_run() {
    let lab = Lab::new("results", true);
    let run = lab.run();
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];
    let list = lab.get("/api/v1/runs", &auth).text();
    let input = lab.dir.join("project/model.inp");
    assert!(
        list.lines()
            .any(|l| l == format!("{run} succeeded {}", input.display())),
        "{list}"
    );
    let base = format!("/api/v1/runs/{run}/files");
    let pvd = lab.get(&format!("{base}/results.pvd"), &auth);
    assert_eq!(pvd.status, 200);
    assert!(pvd.text().contains("file=\"step-1.vtu\""));
    let vtu = lab.get(&format!("{base}/step-1.vtu"), &auth);
    let out = lab.dir.join("state/runs").join(&run).join("out");
    let bytes = fs::read(out.join("step-1.vtu")).unwrap();
    assert_eq!(vtu.body, bytes, "binary bytes pass unchanged");
    assert_eq!(vtu.header("Content-Type"), Some("application/octet-stream"));
    let copy = lab.get(&format!("/api/v1/runs/{run}/input"), &auth);
    let tet = fs::read(fixture("tet.inp")).unwrap();
    assert_eq!((copy.status, copy.body), (200, tet));

    let missing = lab.get(&format!("{base}/step-2.vtu"), &auth);
    assert_eq!(
        (missing.status, missing.error().as_deref()),
        (404, Some("file.missing"))
    );
    for name in [".hidden", "..", "a%2Fb", "x%20y"] {
        let bad = lab.get(&format!("{base}/{name}"), &auth);
        assert_eq!(bad.error().as_deref(), Some("file.invalid"), "{name}");
    }
    for path in [
        "/api/v1/runs/run-999999/files/results.pvd",
        "/api/v1/runs/../input",
    ] {
        assert_eq!(
            lab.get(path, &auth).error().as_deref(),
            Some("run.unknown"),
            "{path}"
        );
    }

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
fn the_page_shows_a_copy_of_the_model_before_the_solve() {
    let lab = Lab::new("preview", false);
    let c = lab.daemon.client();
    let input = "** units: SI-mm\n*NODE\n1, 0, 0, 0\n";
    let file = lab.dir.join("project/model.inp");
    fs::write(&file, input).unwrap();
    let preview = c.preview(&file, "a case").unwrap();
    assert!(c.runs().unwrap().is_empty(), "a preview solves nothing");
    // A later edit of the file does not change the preview.
    fs::write(&file, "*NODE\n").unwrap();
    let path = format!("/api/v1/previews/{preview}");
    let bearer = lab.bearer();
    let own = format!("http://127.0.0.1:{}", lab.daemon.port());
    let page = [("Authorization", bearer.as_str()), ("Origin", own.as_str())];
    let answer = lab.get(&path, &page);
    assert_eq!(
        (answer.status, answer.body.as_slice()),
        (200, input.as_bytes())
    );

    // The token and the Origin rules hold, and only a preview is read.
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
    for id in ["x", "..", "7"] {
        let answer = lab.get(&format!("/api/v1/previews/{id}"), &auth);
        assert_eq!(answer.error().as_deref(), Some("preview.unknown"), "{id}");
    }
    let relative = c.preview(Path::new("model.inp"), "x").unwrap_err();
    assert_eq!(relative.code, "input.invalid");
}

#[test]
fn a_preview_and_a_run_serve_the_model_with_its_include_files() {
    let lab = Lab::new("include", false);
    let c = lab.daemon.client();
    let project = lab.dir.join("project");
    let tet = fs::read_to_string(fixture("tet.inp")).unwrap();
    let (mesh, rest) = tet.split_at(tet.find("*MATERIAL").unwrap());
    fs::write(project.join("mesh.inp"), mesh).unwrap();
    let file = project.join("job.inp");
    fs::write(&file, format!("*INCLUDE, INPUT=mesh.inp\n{rest}")).unwrap();
    let bearer = lab.bearer();
    let auth = [("Authorization", bearer.as_str())];
    let flat = format!("{mesh}{rest}");

    // The page reads one text, with the lines of the include file in place.
    let preview = c.preview(&file, "job").unwrap();
    let shown = lab.get(&format!("/api/v1/previews/{preview}"), &auth);
    assert_eq!((shown.status, shown.text()), (200, flat.clone()));

    let run = c.submit(&file, &Limits::default()).unwrap().run;
    let deadline = Instant::now() + Duration::from_secs(30);
    while c.record(&run).unwrap().state != RunState::Succeeded {
        assert!(Instant::now() < deadline, "the run does not succeed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let copy = lab.get(&format!("/api/v1/runs/{run}/input"), &auth);
    assert_eq!((copy.status, copy.text()), (200, flat));
    let kept = lab.dir.join("state/runs").join(&run).join("in/mesh.inp");
    assert_eq!(fs::read_to_string(kept).unwrap(), mesh);

    // An include path that leaves the folder is refused.
    fs::write(&file, "*INCLUDE, INPUT=../mesh.inp\n").unwrap();
    assert_eq!(
        c.submit(&file, &Limits::default()).unwrap_err().code,
        "input.invalid"
    );
}
