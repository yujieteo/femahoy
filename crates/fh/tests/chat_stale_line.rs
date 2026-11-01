//! After Stop, no entry of the stopped agent shows. A fake agent prints a `result` event when it
//! gets SIGTERM, as an agent can flush its last event. A late `end` entry after "stopped the
//! agent" hides the Rerun button of the page. This file holds one test, because it sets
//! environment variables.
mod common;
use common::{p, scratch, script};

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

use fh::daemon::{Daemon, Options};

const AGENT: &str = "#!/bin/sh\nIFS= read -r line\n\
    trap 'echo \"{\\\"type\\\":\\\"result\\\",\\\"subtype\\\":\\\"success\\\",\\\"is_error\\\":false}\"; exit 0' TERM\n\
    echo '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"meshing\"}]}}'\n\
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
fn no_entry_of_a_stopped_agent_shows() {
    let dir = scratch("chat-stale-line");
    let ready = dir.join("ready");
    script(&dir.join("agent"), &AGENT.replace("READY", p(&ready)));
    std::env::set_var("FH_AGENT", dir.join("agent"));
    let daemon = Daemon::start_with_options(&dir.join("state"), Options::default()).unwrap();

    assert!(send(&daemon, "POST", "/api/v1/chat", "mesh the bracket").starts_with("HTTP/1.1 200"));
    let end = Instant::now() + Duration::from_secs(20);
    while !ready.exists() {
        assert!(Instant::now() < end, "the agent did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(send(&daemon, "POST", "/api/v1/chat/stop", "").starts_with("HTTP/1.1 200"));
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        let chat = send(&daemon, "GET", "/api/v1/chat?since=0", "");
        let after = chat.split("note stopped the agent").nth(1).unwrap_or("");
        assert!(
            !after.contains(" end "),
            "an entry of the stopped agent shows:\n{chat}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(daemon);
    let _ = std::fs::remove_dir_all(&dir);
}
