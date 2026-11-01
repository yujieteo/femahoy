//! Target: femahoy `crates/fh/tests/chat_write_failure.rs`. Pins the row `* | ask | else | none` of
//! `Proposal chat` in `chat.sm.md`, for the refactor of the flags of `Log` into one state.
//! A message that cannot reach the agent ends that agent: the chat is idle, and the next message
//! starts a new agent with no false "ended before the end of its work" error. Today the write
//! failure only drops the input (`daemon/chat.rs:264`): the chat stays `running` with no way to
//! reach the agent (the hidden state `broken`). This file holds one test, because it sets
//! environment variables.
mod common;
use common::{p, scratch, script};

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

use fh::daemon::{Daemon, Options};

/// The agent reads 1 message, closes its stdin, starts a turn and keeps working.
const AGENT: &str = "#!/bin/sh\nIFS= read -r line\nexec 0<&-\n\
    echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"solving\"}]}}'\n\
    touch \"READY\"\nwhile :; do sleep 0.05; done\n";

fn send(daemon: &Daemon, method: &str, path: &str, body: &str) -> String {
    let port = daemon.port();
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\n\
         Content-Length: {}\r\n\r\n{body}",
        daemon.token(),
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    answer
}

#[test]
fn a_message_that_cannot_reach_the_agent_ends_it() {
    let dir = scratch("chat-write-failure");
    let ready = dir.join("ready");
    script(&dir.join("agent"), &AGENT.replace("READY", p(&ready)));
    std::env::set_var("FH_AGENT", dir.join("agent"));
    let daemon = Daemon::start_with_options(&dir.join("state"), Options::default()).unwrap();

    assert!(send(&daemon, "POST", "/api/v1/chat", "solve the plate").starts_with("HTTP/1.1 200"));
    let end = Instant::now() + Duration::from_secs(20);
    while !ready.exists() {
        assert!(Instant::now() < end, "the agent did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    let answer = send(&daemon, "POST", "/api/v1/chat", "and the second mesh");
    assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    let chat = send(&daemon, "GET", "/api/v1/chat?since=0", "");
    assert!(
        chat.contains("state=idle"),
        "the chat must be idle after a write failure:\n{chat}"
    );
    std::fs::remove_file(&ready).unwrap();
    assert!(send(&daemon, "POST", "/api/v1/chat", "again").starts_with("HTTP/1.1 200"));
    let chat = send(&daemon, "GET", "/api/v1/chat?since=0", "");
    assert!(
        !chat.contains("ended before the end of its work"),
        "a false error shows:\n{chat}"
    );
    drop(daemon);
    let _ = std::fs::remove_dir_all(&dir);
}
