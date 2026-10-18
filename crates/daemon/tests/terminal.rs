//! The terminal sidebar and the pane backends (DESIGN.md section 4.3): the token and
//! `Origin` rules on each terminal and pane route and on the WebSocket, a shell over the
//! WebSocket, the PTY panes, and the herdr backend with a fake herdr. The test
//! `herdr_lab_session` uses the real herdr only on a lab session (`FH_TEST_HERDR_SESSION`).

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fh_daemon::screen::{self, input_message, resize_message};
use fh_daemon::{Client, Config, Daemon, Error, Message, Options, PaneChoice, HERDR_COMMANDS};
use fh_solver_api::SubprocessSolver;

/// A test folder that goes away when the test ends.
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

/// Start a daemon. A script that a test has just written can give `Text file busy` on
/// Linux while another test forks a PTY child, so a start tries again for that error.
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

/// Send `route` ("GET /path" or "POST /path"), with `fields` as the body of a POST.
fn call(c: &Client, route: &str, fields: &[(&str, &str)]) -> Result<Message, Error> {
    let (method, path) = route.split_once(' ').unwrap();
    let mut m = Message::new();
    fields.iter().for_each(|(k, v)| m.set(k, v));
    let body = (method == "POST").then_some(&m);
    c.send(method, path, body, Duration::ZERO)
}

/// The error code of an answer, or `ok`.
fn code<T>(r: Result<T, Error>) -> String {
    r.map_or_else(|e| e.code, |_| "ok".into())
}

/// Poll `GET path` until its items have `want`.
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

/// A raw request with `headers`, and the status and error code of the answer.
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
        // The subprotocol carries the token only on the WebSocket route. There it passes
        // the access rules, and then the terminal t1 does not exist.
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
    // No refused request opened a terminal.
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
    // The token in the subprotocol, as a browser sends it, from the page origin.
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
    // The head names the new size: 60 columns and 10 rows.
    assert_eq!(&m[1..5], &[0, 60, 0, 10]);
    wait_text(&c, &format!("/api/v1/terminals/{id}/text"), "fh-42");
    // A browser that goes stops nothing, and a new one gets the full screen.
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
    assert_eq!(
        (list.get("panes"), list.get("herdr")),
        (Some("pty"), Some("-"))
    );
}

#[test]
fn pty_panes_run_commands_and_take_keys() {
    let d = dir("pty");
    let daemon = start(&d, Options::default()).unwrap();
    let c = daemon.client();
    let status = call(&c, "GET /api/v1/panes", &[]).unwrap();
    let got = (status.get("backend"), status.get("running"));
    assert_eq!(got, (Some("pty"), Some("yes")));
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
    // A shell terminal is not a pane, and the pty backend has no herdr.
    let t = call(&c, "POST /api/v1/terminals", &[("kind", "shell")]).unwrap();
    let shell = format!("GET /api/v1/panes/{}", t.need("terminal").unwrap());
    assert_eq!(code(call(&c, &shell, &[])), "pane.unknown");
    let herdr = call(&c, "POST /api/v1/terminals", &[("kind", "herdr")]).unwrap_err();
    assert_eq!(
        (herdr.status, herdr.code.as_str()),
        (409, "term.herdr.absent")
    );
    let zsh = call(&c, "POST /api/v1/terminals", &[("kind", "zsh")]);
    assert_eq!(code(zsh), "term.kind");
}

/// A fake herdr: it logs each call and answers as herdr 0.9 does, with an error as JSON on
/// stderr.
const FAKE_HERDR: &str = r#"#!/bin/sh
echo "$* | session=$HERDR_SESSION others=$(env | grep ^HERDR_ | grep -vc ^HERDR_SESSION=)" >> "$0.log"
[ "$1 $2" = "--session fh-test" ] || { echo '{"error":{"code":"x","message":"no session"}}' >&2; exit 1; }
shift 2
case "$1 $2" in
"status --json") echo '{"client":{"version":"9.9.9"},"server":{"running":true}}' ;;
"workspace list") [ -e "$0.ws" ] && echo '{"result":{"workspaces":[{"label":"femahoy","workspace_id":"w1"}]}}' || echo '{"result":{"workspaces":[]}}' ;;
"workspace create") sleep 0.3; touch "$0.ws"; echo '{"result":{"root_pane":{"pane_id":"w1:p1"}}}' ;;
"tab create") echo '{"result":{"root_pane":{"pane_id":"w1:p2"}}}' ;;
"pane get") case "$3" in w1:p9) echo '{"error":{"code":"pane_not_found","message":"pane w1:p9 not found"}}' >&2; exit 1 ;; w1:*) echo "{\"result\":{\"pane\":{\"pane_id\":\"$3\",\"workspace_id\":\"w1\"}}}" ;; *) echo '{"result":{"pane":{"pane_id":"w2:p1","workspace_id":"w2"}}}' ;; esac ;;
"pane list") echo '{"result":{"panes":[{"pane_id":"w1:p1","label":"one"},{"pane_id":"w1:p2","label":"two"}]}}' ;;
"pane read") printf 'line 1\nline 2\n' ;;
"pane rename"|"pane run"|"pane send-text"|"pane send-keys"|"pane close") echo '{"result":{"type":"ok"}}' ;;
*) echo '{"error":{"code":"unknown_command","message":"nope"}}' >&2; exit 1 ;;
esac
"#;

/// The options for the herdr program `script` (written to `d/herdr`) on `session`.
fn herdr(d: &Path, script: &str, session: &str, panes: PaneChoice) -> Options {
    let program = d.join("herdr");
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let mut options = Options {
        panes,
        ..Options::default()
    };
    (options.herdr_session, options.herdr_program) = (session.into(), Some(program));
    options
}

#[test]
fn the_herdr_backend_calls_only_its_own_session() {
    let d = dir("herdr");
    let fake = |session| herdr(&d, FAKE_HERDR, session, PaneChoice::Herdr);
    assert_eq!(code(start(&d, fake("default"))), "pane.herdr.session");
    let daemon = start(&d, fake("fh-test")).unwrap();
    let c = daemon.client();
    let status = call(&c, "GET /api/v1/panes", &[]).unwrap();
    assert_eq!(status.get("backend"), Some("herdr"));
    let got = (status.get("session"), status.get("detail"));
    assert_eq!(got, (Some("fh-test"), Some("herdr 9.9.9")));
    // The page reads the terminal list often, so it calls no herdr.
    let calls = || {
        fs::read_to_string(d.join("herdr.log"))
            .unwrap()
            .lines()
            .count()
    };
    let before = calls();
    let list = call(&c, "GET /api/v1/terminals", &[]).unwrap();
    let got = (list.get("panes"), list.get("herdr"));
    assert_eq!((got, calls()), ((Some("herdr"), Some("fh-test")), before));
    let cwd = d.display().to_string();
    let open = |title: &str| {
        let fields = [("title", title), ("cwd", cwd.as_str()), ("command", "make")];
        call(&c, "POST /api/v1/panes", &fields).unwrap()
    };
    assert_eq!(open("one").get("pane"), Some("w1:p1"));
    assert_eq!(open("two").get("pane"), Some("w1:p2"));
    let list = call(&c, "GET /api/v1/panes", &[]).unwrap().items;
    assert_eq!(list, ["w1:p1 running - one", "w1:p2 running - two"]);
    let text = call(&c, "GET /api/v1/panes/w1:p1/text?lines=5", &[]).unwrap();
    assert_eq!(text.items, ["line 1", "line 2"]);
    let keys = [("text", "ls"), ("keys", "enter ctrl+c")];
    call(&c, "POST /api/v1/panes/w1:p1/send", &keys).unwrap();
    call(&c, "POST /api/v1/panes/w1:p2/close", &[]).unwrap();
    // A pane of another workspace is not ours, and a bad pane ID never reaches herdr.
    let foreign = call(&c, "GET /api/v1/panes/w2:p1", &[]).unwrap_err();
    assert_eq!(
        (foreign.status, foreign.code.as_str()),
        (404, "pane.unknown")
    );
    let bad = call(&c, "GET /api/v1/panes/--help", &[]);
    assert_eq!(code(bad), "pane.unknown");
    let gone = call(&c, "GET /api/v1/panes/w1:p9", &[]);
    assert_eq!(code(gone), "pane.unknown");
    // The herdr client of the sidebar runs on the session of femahoy.
    let t = call(&c, "POST /api/v1/terminals", &[("kind", "herdr")]).unwrap();
    assert_eq!(t.get("kind"), Some("herdr"));
    wait_text(&c, "/api/v1/terminals", "exited");
    // Each call names the session and gets no HERDR_* variable of the daemon. Each API
    // call is an allowed command. The client of the sidebar has no other word.
    let log = fs::read_to_string(d.join("herdr.log")).unwrap();
    for line in log.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        assert_eq!(words[..2], ["--session", "fh-test"], "{line}");
        assert!(line.ends_with(" others=0"), "{line}");
        let api = HERDR_COMMANDS.contains(&words[2..4].join(" ").as_str());
        assert!(
            api && line.contains("session=fh-test") || words[2] == "|",
            "{line}"
        );
    }
    let tab = format!("tab create --workspace w1 --label two --cwd {cwd} --no-focus |");
    for call in [
        "workspace create --label femahoy --cwd",
        tab.as_str(),
        "pane run w1:p1 make |",
        "pane read w1:p1 --source recent-unwrapped --lines 5 |",
        "pane send-keys w1:p1 enter ctrl+c |",
        "pane close w1:p2 |",
        "--session fh-test | session",
    ] {
        assert!(log.contains(call), "no {call:?} in\n{log}");
    }
    // `auto` takes the pty backend when herdr does not answer. `herdr` fails at start.
    drop(daemon);
    let broken = |panes| herdr(&d, "#!/bin/sh\nexit 1\n", "femahoy", panes);
    let auto = start(&d, broken(PaneChoice::Auto)).unwrap();
    assert_eq!(auto.panes().status().kind, "pty");
    drop(auto);
    assert_eq!(code(start(&d, broken(PaneChoice::Herdr))), "pane.herdr");
}

#[test]
fn the_session_name_is_checked_for_each_backend() {
    let d = dir("session");
    for panes in [PaneChoice::Pty, PaneChoice::Auto, PaneChoice::Herdr] {
        let options = Options {
            panes,
            herdr_session: "default".into(),
            ..Options::default()
        };
        assert_eq!(code(start(&d, options)), "pane.herdr.session", "{panes:?}");
    }
}

#[test]
fn two_opens_at_once_make_one_workspace() {
    let d = dir("race");
    let daemon = start(&d, herdr(&d, FAKE_HERDR, "fh-test", PaneChoice::Herdr)).unwrap();
    let cwd = d.display().to_string();
    let ids: Vec<String> = std::thread::scope(|s| {
        ["one", "two"]
            .map(|title| {
                let (c, cwd) = (daemon.client(), cwd.clone());
                s.spawn(move || {
                    let fields = [("title", title), ("cwd", cwd.as_str())];
                    let p = call(&c, "POST /api/v1/panes", &fields).unwrap();
                    p.need("pane").unwrap().to_owned()
                })
            })
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect()
    });
    assert_eq!(ids.len(), 2);
    let log = fs::read_to_string(d.join("herdr.log")).unwrap();
    let made = log
        .lines()
        .filter(|l| l.contains("workspace create"))
        .count();
    assert_eq!(made, 1, "{log}");
}

/// The real herdr, only on a lab session that the caller provisioned and tears down.
#[test]
#[ignore = "needs FH_TEST_HERDR_SESSION, a provisioned fm-lab- session (docs/terminal.md)"]
fn herdr_lab_session() {
    let session = std::env::var("FH_TEST_HERDR_SESSION")
        .expect("set FH_TEST_HERDR_SESSION to a provisioned fm-lab- session (docs/terminal.md)");
    assert!(
        session.starts_with("fm-lab-"),
        "only a lab session, never {session:?}"
    );
    let d = dir("lab");
    let mut options = Options {
        panes: PaneChoice::Herdr,
        ..Options::default()
    };
    options.herdr_session = session.clone();
    options.herdr_program = std::env::var_os("FH_TEST_HERDR_PROGRAM").map(PathBuf::from);
    let daemon = start(&d, options).unwrap();
    let c = daemon.client();
    let status = call(&c, "GET /api/v1/panes", &[]).unwrap();
    let got = (status.get("session"), status.get("running"));
    assert_eq!(got, (Some(session.as_str()), Some("yes")));
    let cwd = d.display().to_string();
    let fields = [
        ("title", "lab"),
        ("cwd", &cwd),
        ("command", "echo fh-lab-$((40+2))"),
    ];
    let p = call(&c, "POST /api/v1/panes", &fields).unwrap();
    let id = p.need("pane").unwrap().to_owned();
    wait_text(&c, &format!("/api/v1/panes/{id}/text"), "fh-lab-42");
    let keys = [("text", "echo typed-$((1+1))"), ("keys", "enter")];
    call(&c, &format!("POST /api/v1/panes/{id}/send"), &keys).unwrap();
    wait_text(&c, &format!("/api/v1/panes/{id}/text"), "typed-2");
    wait_text(&c, "/api/v1/panes", &format!("{id} running - lab"));
    call(&c, &format!("POST /api/v1/panes/{id}/close"), &[]).unwrap();
    let gone = call(&c, &format!("GET /api/v1/panes/{id}"), &[]);
    assert_eq!(code(gone), "pane.unknown");
}
