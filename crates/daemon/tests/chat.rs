//! The chat routes with a fake agent that prints a recorded event stream of `claude -p
//! --input-format stream-json --output-format stream-json`. No test calls a real LLM. The
//! recording is in `tests/data/agent-1.jsonl` (to the `fh run` command) and
//! `tests/data/agent-2.jsonl` (its result and the end). The fake takes each message of the
//! person from 1 line of stdin and replays it, as `--replay-user-messages` does. It takes
//! the second message during its work, and prints the second part when the file `go`
//! exists, after the test solved a run on the daemon, as `fh run` does.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use fh_daemon::{Client, Config, Daemon, Error, Message, Options, RunState};
use fh_solver_api::SubprocessSolver;

/// The fake solver. A model with `FAIL` in it fails. Another model solves when the file
/// `release` exists.
fn solver(dir: &Path) -> String {
    format!(
        r#"#!/bin/sh
echo "fh-solver-protocol 2"
echo "progress step=1/1 phase=solve fraction=0.5"
if grep -q FAIL "$1"; then
  echo "the conjugate gradient solver did not reach the tolerance" >&2
  exit 3
fi
while [ ! -e '{}' ]; do sleep 0.05; done
printf '<VTKFile type="Collection"><Collection units="SI-mm"><DataSet timestep="1" part="0" file="step-1.vtu"/></Collection></VTKFile>\n' > results.pvd
printf 'VTU' > step-1.vtu
printf 'schema: fh.run\nversion: 1.1.0\nstatus: ok\n' > summary.toon
"#,
        dir.join("release").display()
    )
}

/// The fake agent. It keeps each message, the arguments and the environment that it gets.
/// After the recording, it takes 1 more message and works until a stop.
fn agent(data: &Path) -> String {
    let (one, two) = (data.join("agent-1.jsonl"), data.join("agent-2.jsonl"));
    format!(
        r#"#!/bin/sh
printf '%s\n' "$@" > args.txt
printf 'FH_STATE=%s\nFH_NO_OPEN=%s\n' "$FH_STATE" "$FH_NO_OPEN" > env.txt
take() {{
  IFS= read -r line || exit 0
  printf '%s\n' "$line" >> prompt.txt
  echo '{{"type":"user","isReplay":true,"message":{{"role":"user","content":"taken"}}}}'
}}
take
cat '{}'
take
while [ ! -e go ]; do sleep 0.05; done
token=$(sed -n 's/^token=//p' "$FH_STATE/daemon.info")
sed "s/@TOKEN@/$token/" '{}'
take
echo '{{"type":"assistant","message":{{"content":[{{"type":"text","text":"I work."}}]}}}}'
while :; do sleep 0.05; done
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
    for (name, text) in [("solver.sh", solver(&dir)), ("agent.sh", agent(&data))] {
        fs::write(dir.join(name), text).unwrap();
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The only test of this file, so no other test sees the variable.
    std::env::set_var("FH_AGENT", dir.join("agent.sh"));
    let state = dir.join("state");
    let config = Config {
        state_dir: state.clone(),
        solver: SubprocessSolver::new(dir.join("solver.sh")),
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
    wait(&c, |m| m.items.iter().any(|i| i.contains(" cmd fh run ")));

    // A message during the work goes to the agent at once, and the agent continues.
    let chat = state.join("chat");
    let redirect = "Use a finer mesh.";
    let m = call(&c, "POST /api/v1/chat", &[("text", redirect)]).unwrap();
    assert_eq!(m.get("state"), Some("running"));
    let prompts = || fs::read_to_string(chat.join("prompt.txt")).unwrap_or_default();
    let deadline = Instant::now() + Duration::from_secs(30);
    while prompts().lines().count() < 2 {
        assert!(
            Instant::now() < deadline,
            "the agent did not take the message"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let line = |text: &str| {
        format!(
            "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}},\"parent_tool_use_id\":null}}"
        )
    };
    assert_eq!(
        prompts(),
        format!("{}\n{}\n", line(request), line(redirect))
    );

    // The `fh run` of the agent sends the model to the page, then solves it. While it
    // solves, the chat shows the run and its progress, and the time when the work started.
    let (model, bad) = (dir.join("project/model.inp"), dir.join("project/bad.inp"));
    fs::write(&model, "*NODE\n").unwrap();
    fs::write(&bad, "*NODE\n** FAIL\n").unwrap();
    let preview = c.preview(&model, "my case").unwrap();
    let bad = c.submit(&bad).unwrap().run;
    let run = c.submit(&model).unwrap().run;
    let solving = format!("{run} running progress step=1/1 phase=solve fraction=0.5");
    let m = wait(&c, |m| m.get("solving") == Some(solving.as_str()));
    assert_eq!(m.get("state"), Some("running"));
    let started: u64 = m.get("started_ms").unwrap().parse().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    assert!(started > 0 && started <= now.as_millis() as u64, "{m:?}");
    fs::write(dir.join("release"), "").unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let record = c.record(&run).unwrap();
        if record.state == RunState::Succeeded {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the run does not succeed: {record:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(c.record(&bad).unwrap().state, RunState::Failed);
    fs::write(chat.join("go"), "").unwrap();
    let m = wait(&c, |m| m.get("state") == Some("idle"));
    assert_eq!((m.get("solving"), m.get("started_ms")), (None, None));

    let kinds: Vec<&str> = m
        .items
        .iter()
        .map(|i| i.split(' ').nth(1).unwrap())
        .collect();
    let first = |kind: &str| kinds.iter().position(|k| *k == kind).unwrap();
    let last = |kind: &str| kinds.iter().rposition(|k| *k == kind).unwrap();
    assert_eq!(kinds[0], "you");
    assert!(
        first("cmd") < last("you") && last("you") < first("show") && first("show") < first("open"),
        "{kinds:?}"
    );
    assert!(
        first("failed") < first("end") && first("open") < first("end"),
        "{kinds:?}"
    );
    let text = m.items.join("\n");
    assert!(text.contains(&format!(" show {preview} my case")), "{text}");
    assert!(text.contains(&format!(" open {run}")), "{text}");
    // A failed solve is an entry with its error.
    let failed = m.items.iter().find(|i| i.contains(" failed ")).unwrap();
    assert!(
        failed.contains(&format!(" failed {bad} "))
            && failed.contains("did not reach the tolerance"),
        "{failed}"
    );
    assert!(text.contains(" end done in "), "{text}");
    assert!(text.contains("USD"), "{text}");
    assert!(text.contains("<token>") && !text.contains(&token), "{text}");
    // Each entry has its sequence number, and `since` gives the later entries only.
    let next: u64 = m.get("next").unwrap().parse().unwrap();
    assert_eq!(next as usize, m.items.len());
    let later = call(&c, &format!("GET /api/v1/chat?since={}", next - 1), &[]).unwrap();
    assert_eq!(later.items.len(), 1);

    // The agent got the stream flags and this daemon.
    let args = fs::read_to_string(chat.join("args.txt")).unwrap();
    for flag in [
        "-p",
        "--input-format\nstream-json\n",
        "--output-format\nstream-json\n",
        "--replay-user-messages",
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

    // The next request goes to the same agent. Stop ends the agent at once.
    call(&c, "POST /api/v1/chat", &[("text", "Again.")]).unwrap();
    wait(&c, |m| m.items.iter().any(|i| i.ends_with(" say I work.")));
    assert_eq!(prompts().lines().count(), 3);
    call(&c, "POST /api/v1/chat/stop", &[]).unwrap();
    let m = wait(&c, |m| m.get("state") == Some("idle"));
    assert!(
        m.items
            .iter()
            .any(|i| i.ends_with(" note stopped the agent")),
        "{m:?}"
    );
    // After a stop, a new agent continues the session.
    call(&c, "POST /api/v1/chat", &[("text", "Continue.")]).unwrap();
    wait(&c, |m| {
        m.items.iter().any(|i| i.ends_with(" you Continue."))
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !fs::read_to_string(chat.join("args.txt"))
        .unwrap()
        .contains("--resume\n")
    {
        assert!(Instant::now() < deadline, "the new agent does not resume");
        std::thread::sleep(Duration::from_millis(20));
    }
    call(&c, "POST /api/v1/chat/stop", &[]).unwrap();
    let m = wait(&c, |m| m.get("state") == Some("idle"));
    assert!(
        !m.items.iter().any(|i| i.split(' ').nth(1) == Some("error")),
        "a stop is not a failure: {m:?}"
    );

    // An agent that ends during its work is an error entry.
    let broken = dir.join("broken.sh");
    fs::write(
        &broken,
        "#!/bin/sh\nread -r line\necho '{\"type\":\"user\",\"isReplay\":true}'\necho 'the agent lost its connection' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&broken, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var("FH_AGENT", &broken);
    call(&c, "POST /api/v1/chat", &[("text", "Continue.")]).unwrap();
    let m = wait(&c, |m| {
        m.get("state") == Some("idle") && m.items.iter().any(|i| i.contains("lost its"))
    });
    assert!(
        m.items.iter().any(|i| i
            .ends_with(" error the agent ended before the end of its work (exit code Some(1))")),
        "{m:?}"
    );
    drop(daemon);
    let _ = fs::remove_dir_all(&dir);
}
