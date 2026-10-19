//! The Stop of the chat stops ccx. A fake agent takes the first message, runs `fh run` as the
//! Bash tool of the real agent does, and passes SIGTERM on to it. The Stop button of the page
//! sends SIGTERM to the process group of the agent. This file holds one test, because it sets
//! environment variables.
mod common;
use common::{p, root, scratch, script};

use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::process::Command;
use std::time::{Duration, Instant};

use fh_daemon::{Daemon, Options};

/// The ccx waits and ignores SIGTERM, so only a kill of its process group stops it.
const CCX: &str = "#!/bin/sh\ntrap '' TERM\nsleep 300 &\n\
    echo $$ $! > PIDS.tmp\nmv PIDS.tmp PIDS\nwait\n";

const AGENT: &str = "#!/bin/sh\nIFS= read -r line\n\"FH\" run \"MODEL\" &\nchild=$!\n\
    trap 'kill -TERM $child; wait $child; exit 0' TERM\nwait $child\n";

fn post(daemon: &Daemon, path: &str, body: &str) -> String {
    let port = daemon.port();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\n\
         Content-Length: {}\r\n\r\n{body}",
        daemon.token(),
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}

fn alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn the_stop_of_the_chat_stops_ccx() {
    let dir = scratch("chat-stop");
    let pids = dir.join("pids");
    script(&dir.join("ccx"), &CCX.replace("PIDS", p(&pids)));
    let model = root().join("validation/cases/bar-pressure/job.inp");
    let agent = AGENT
        .replace("FH", env!("CARGO_BIN_EXE_fh"))
        .replace("MODEL", p(&model));
    script(&dir.join("agent"), &agent);
    std::env::set_var("FH_AGENT", dir.join("agent"));
    std::env::set_var("FH_CCX", dir.join("ccx"));
    let daemon = Daemon::start_with_options(&dir.join("state"), Options::default()).unwrap();

    let answer = post(&daemon, "/api/v1/chat", "text=solve it\n");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    let end = Instant::now() + Duration::from_secs(30);
    let started = loop {
        if let Ok(text) = fs::read_to_string(&pids) {
            break text
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
        }
        assert!(Instant::now() < end, "ccx did not start");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(started.len(), 2, "{started:?}");

    let answer = post(&daemon, "/api/v1/chat/stop", "");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    while started.iter().any(|pid| alive(pid)) {
        assert!(Instant::now() < end, "ccx runs after the stop: {started:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(daemon);
    let _ = fs::remove_dir_all(&dir);
}
