//! The chat routes with a fake agent that prints a recorded event stream of `claude -p
//! --output-format stream-json`. No test calls a real LLM. The recording is in
//! `tests/data/agent-1.jsonl` (to the `fh run` command) and `tests/data/agent-2.jsonl` (its
//! result and the end). The fake prints the second part when the file `go` exists, after
//! the test solved a run on the daemon, as `fh run` does.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use fh_daemon::{new_request_id, Client, Config, Daemon, Error, Message, Options, RunState};
use fh_solver_api::SubprocessSolver;

const SOLVER: &str = r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=1"
printf '<VTKFile type="Collection"><Collection units="SI-mm"><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>\n' > results.pvd
printf 'VTU' > step-1.vtu
printf 'schema: fh.run\nversion: 1.1.0\nstatus: ok\n' > summary.toon
"#;

/// The fake agent. It keeps the request, the arguments and the environment that it gets.
fn agent(data: &Path) -> String {
    let (one, two) = (data.join("agent-1.jsonl"), data.join("agent-2.jsonl"));
    format!(
        r#"#!/bin/sh
cat > prompt.txt
printf '%s\n' "$@" > args.txt
printf 'FH_STATE=%s\nFH_NO_OPEN=%s\n' "$FH_STATE" "$FH_NO_OPEN" > env.txt
cat '{}'
while [ ! -e go ]; do sleep 0.05; done
token=$(sed -n 's/^token=//p' "$FH_STATE/daemon.info")
sed "s/@TOKEN@/$token/" '{}'
"#,
        one.display(),
        two.display()
    )
}

fn call(c: &Client, route: &str, fields: &[(&str, &str)]) -> Result<Message, Error> {
    let (method, path) = route.split_once(' ').unwrap();
    let mut m = Message::new();
    fields.iter().for_each(|(k, v)| m.set(k, v));
    c.send(
        method,
        path,
        (method == "POST").then_some(&m),
        Duration::ZERO,
    )
}

fn code<T>(r: Result<T, Error>) -> String {
    r.map_or_else(|e| e.code, |_| "ok".into())
}

/// Read the chat until `done` is true for the answer.
fn wait(c: &Client, done: impl Fn(&Message) -> bool) -> Message {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let m = call(c, "GET /api/v1/chat?since=0", &[]).unwrap();
        if done(&m) {
            return m;
        }
        assert!(Instant::now() < deadline, "the chat did not change: {m:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_chat_shows_each_action_of_the_agent_and_opens_the_new_run() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fh-chat-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("project")).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    for (name, text) in [("solver.sh", SOLVER.to_owned()), ("agent.sh", agent(&data))] {
        fs::write(dir.join(name), text).unwrap();
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The only test of this file, so no other test sees the variable.
    std::env::set_var("FH_AGENT", dir.join("agent.sh"));
    let state = dir.join("state");
    let config = Config {
        state_dir: state.clone(),
        solver: SubprocessSolver::new(dir.join("solver.sh")),
        solver_dirs: Vec::new(),
    };
    let daemon = Daemon::start_with_options(config, 0, Options::default()).unwrap();
    let c = daemon.client();
    let token = daemon.token().to_owned();

    // The chat routes have the token and Origin rules of the other routes.
    let no_token = Client::new(daemon.port(), "");
    assert_eq!(
        code(call(&no_token, "GET /api/v1/chat", &[])),
        "token.missing"
    );
    let foreign = daemon.client().with_origin("http://evil.example");
    assert_eq!(
        code(call(&foreign, "POST /api/v1/chat", &[("text", "hi")])),
        "origin.refused"
    );
    assert_eq!(
        code(call(&c, "POST /api/v1/chat", &[("text", " ")])),
        "chat.empty"
    );

    let request = "Build and solve a steel cantilever of 100 x 10 x 10 mm.";
    let m = call(&c, "POST /api/v1/chat", &[("text", request)]).unwrap();
    assert_eq!(m.get("state"), Some("running"));
    assert_eq!(
        code(call(&c, "POST /api/v1/chat", &[("text", "again")])),
        "chat.busy"
    );
    wait(&c, |m| m.items.iter().any(|i| i.contains(" cmd fh run ")));

    // The `fh run` of the agent sends the model to the page, then solves the same revision.
    let project = c.register(&dir.join("project")).unwrap();
    fs::write(dir.join("project/model.inp"), "*NODE\n").unwrap();
    let revision = c.preview(&project, "model.inp", "my case").unwrap();
    let ack = c.submit(&project, &new_request_id(), "model.inp").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let record = c.record(&project, &ack.run).unwrap();
        if record.state == RunState::Succeeded {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the run does not succeed: {record:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(state.join("chat/go"), "").unwrap();
    let m = wait(&c, |m| m.get("state") == Some("idle"));

    let kinds: Vec<&str> = m
        .items
        .iter()
        .map(|i| i.split(' ').nth(1).unwrap())
        .collect();
    let first = |kind: &str| kinds.iter().position(|k| *k == kind).unwrap();
    assert_eq!(kinds[0], "you");
    assert!(
        first("cmd") < first("show") && first("show") < first("open"),
        "{kinds:?}"
    );
    assert!(first("open") < first("end"), "{kinds:?}");
    let text = m.items.join("\n");
    assert!(
        text.contains(&format!(" show {project} {revision} my case")),
        "{text}"
    );
    let record = c.record(&project, &ack.run).unwrap();
    assert_eq!(
        record.revision, revision,
        "the run solves the shown revision"
    );
    assert!(
        text.contains(&format!(" open {project} {}", ack.run)),
        "{text}"
    );
    assert!(text.contains(" end done in "), "{text}");
    assert!(text.contains("USD"), "{text}");
    assert!(text.contains("<token>") && !text.contains(&token), "{text}");
    // Each entry has its sequence number, and `since` gives the later entries only.
    let next: u64 = m.get("next").unwrap().parse().unwrap();
    assert_eq!(next as usize, m.items.len());
    let later = call(&c, &format!("GET /api/v1/chat?since={}", next - 1), &[]).unwrap();
    assert_eq!(later.items.len(), 1);

    // The agent got the request on stdin, the stream flags, and this daemon.
    let chat = state.join("chat");
    assert_eq!(
        fs::read_to_string(chat.join("prompt.txt")).unwrap(),
        request
    );
    let args = fs::read_to_string(chat.join("args.txt")).unwrap();
    for flag in [
        "-p",
        "--output-format\nstream-json\n",
        "--verbose",
        "--allowedTools",
    ] {
        assert!(args.contains(flag), "{args}");
    }
    assert!(!args.contains("--resume"));
    // The CAD script runs with the femahoy Python (fh-python) only.
    assert!(args.contains("Bash(fh-python *)"), "{args}");
    assert!(
        !args.contains("python3") && !args.contains("uv run"),
        "{args}"
    );
    let env = fs::read_to_string(chat.join("env.txt")).unwrap();
    assert!(
        env.contains(&format!("FH_STATE={}\n", state.display())),
        "{env}"
    );
    assert!(env.contains("FH_NO_OPEN=1\n"), "{env}");

    // The next request continues the session. Stop ends the agent.
    fs::remove_file(chat.join("go")).unwrap();
    call(&c, "POST /api/v1/chat", &[("text", "Use a finer mesh.")]).unwrap();
    wait(&c, |m| m.items.len() > next as usize + 2);
    call(&c, "POST /api/v1/chat/stop", &[]).unwrap();
    let m = wait(&c, |m| m.get("state") == Some("idle"));
    assert!(
        m.items
            .iter()
            .any(|i| i.ends_with(" note stopped the agent")),
        "{m:?}"
    );
    let args = fs::read_to_string(chat.join("args.txt")).unwrap();
    assert!(args.contains("--resume\n"), "{args}");
    drop(daemon);
    let _ = fs::remove_dir_all(&dir);
}
