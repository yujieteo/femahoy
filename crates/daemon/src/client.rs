//! The client side of the run contract, for the `fh` CLI and the tests.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::contract::{Error, Events, Message, RunRecord, RunState};
use crate::INFO_FILE;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ANSWER: u64 = 4 << 20;

/// A connection to a daemon on 127.0.0.1.
#[derive(Debug, Clone)]
pub struct Client {
    port: u16,
    token: String,
    origin: Option<String>,
}

fn transport(message: impl ToString) -> Error {
    Error::new(0, "transport", message.to_string())
}

impl Client {
    pub fn new(port: u16, token: impl Into<String>) -> Client {
        Client {
            port,
            token: token.into(),
            origin: None,
        }
    }

    /// The client of the daemon that uses `state_dir`, from its `daemon.info` file.
    pub fn discover(state_dir: &Path) -> Result<Client, Error> {
        let path = state_dir.join(INFO_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::new(0, "daemon.absent", format!("{}: {e}", path.display())))?;
        let m = Message::parse(&text)?;
        let port = m
            .need("port")?
            .parse()
            .map_err(|_| Error::bad("field.invalid", "bad port"))?;
        Ok(Client::new(port, m.need("token")?))
    }

    /// Send an `Origin` header, as a browser does. The daemon accepts only its own page.
    pub fn with_origin(mut self, origin: &str) -> Client {
        self.origin = Some(origin.to_owned());
        self
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The address of the page of the daemon, with the token. `query` follows the token, for
    /// example `&run=R` to open a run.
    pub fn page_url(&self, query: &str) -> String {
        format!(
            "http://127.0.0.1:{}/?token={}{query}",
            self.port, self.token
        )
    }

    /// Send one request and read the answer.
    pub fn send(
        &self,
        method: &str,
        path: &str,
        body: Option<&Message>,
        wait: Duration,
    ) -> Result<Message, Error> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(transport)?;
        stream
            .set_read_timeout(Some(READ_TIMEOUT + wait))
            .map_err(transport)?;
        let body = body.map(Message::to_text).unwrap_or_default();
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {}\r\n\
             Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.port,
            self.token,
            body.len()
        );
        if let Some(origin) = &self.origin {
            head.push_str(&format!("Origin: {origin}\r\n"));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .map_err(transport)?;
        let mut text = String::new();
        stream
            .take(MAX_ANSWER)
            .read_to_string(&mut text)
            .map_err(transport)?;
        let (head, body) = text
            .split_once("\r\n\r\n")
            .ok_or_else(|| transport("the answer has no HTTP head"))?;
        let status: u16 = head
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| transport("the answer has no HTTP status"))?;
        let m = Message::parse(body)?;
        if status == 200 {
            return Ok(m);
        }
        Err(Error::new(
            status,
            m.get("error").unwrap_or("http.error"),
            m.get("message").unwrap_or_default(),
        ))
    }

    fn get(&self, path: &str) -> Result<Message, Error> {
        self.send("GET", path, None, Duration::ZERO)
    }

    fn post(&self, path: &str, fields: &[(&str, &str)]) -> Result<Message, Error> {
        let mut m = Message::new();
        for (k, v) in fields {
            m.set(k, v);
        }
        self.send("POST", path, Some(&m), Duration::ZERO)
    }

    pub fn status(&self) -> Result<Message, Error> {
        self.get("/api/v1/status")
    }

    /// Queue a run of the input file at the absolute path `input`.
    pub fn submit(&self, input: &Path) -> Result<RunRecord, Error> {
        let input = utf8(input)?;
        RunRecord::from_message(&self.post("/api/v1/runs", &[("input", input)])?)
    }

    /// Show the model of the input file at the absolute path `input` with its supports and
    /// loads in the page, with no run. The answer is the preview ID.
    pub fn preview(&self, input: &Path, title: &str) -> Result<String, Error> {
        let fields = [("input", utf8(input)?), ("title", title)];
        let answer = self.post("/api/v1/previews", &fields)?;
        Ok(answer.need("preview")?.to_owned())
    }

    /// Stop a run. The answer is the record after the cancel.
    pub fn cancel(&self, run: &str) -> Result<RunRecord, Error> {
        RunRecord::from_message(&self.post(&format!("/api/v1/runs/{run}/cancel"), &[])?)
    }

    pub fn record(&self, run: &str) -> Result<RunRecord, Error> {
        RunRecord::from_message(&self.get(&format!("/api/v1/runs/{run}"))?)
    }

    /// Each run with its state.
    pub fn runs(&self) -> Result<Vec<(String, RunState)>, Error> {
        let m = self.get("/api/v1/runs")?;
        m.items
            .iter()
            .map(|item| {
                let mut parts = item.splitn(3, ' ');
                let (run, state) = (parts.next(), parts.next().and_then(RunState::parse));
                run.zip(state)
                    .map(|(run, state)| (run.to_owned(), state))
                    .ok_or_else(|| Error::bad("message.invalid", format!("bad run {item}")))
            })
            .collect()
    }

    /// The events after `since`. The daemon waits up to `wait` for a new event.
    pub fn events(&self, run: &str, since: u64, wait: Duration) -> Result<Events, Error> {
        let path = format!(
            "/api/v1/runs/{run}/events?since={since}&wait_ms={}",
            wait.as_millis()
        );
        Events::from_message(&self.send("GET", &path, None, wait)?)
    }
}

fn utf8(path: &Path) -> Result<&str, Error> {
    path.to_str()
        .ok_or_else(|| Error::bad("input.invalid", "the input path must be UTF-8"))
}
