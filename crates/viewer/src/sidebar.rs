//! The terminal sidebar of the page of `fh serve` (DESIGN.md sections 3 and 4.3): one tab for
//! each terminal of the daemon, the buttons for a new shell and for Herdr, and the panes of
//! the pane backend. The daemon owns each terminal, so a tab that the browser closes stops
//! nothing, and a new page shows the same terminals. `docs/terminal.md` has the routes.

use std::collections::BTreeMap;

use eframe::egui;

use crate::terminal::{Connection, Term};

/// The daemon as the sidebar sees it. `web.rs` has the browser one, and the tests a fake.
pub trait Remote {
    /// Send an HTTP request. `body` has the fields in the text form of the run contract. The
    /// answer comes back from [`Remote::poll`] with the same `id`.
    fn request(&mut self, id: u64, method: &str, path: &str, body: &str);
    /// Open the WebSocket of a terminal.
    fn connect(&mut self, terminal: &str);
    fn send(&mut self, terminal: &str, message: &[u8]);
    fn disconnect(&mut self, terminal: &str);
    /// The events since the last call.
    fn poll(&mut self) -> Vec<RemoteEvent>;
}

#[derive(Debug)]
pub enum RemoteEvent {
    /// The answer to request `id`: the text of the message, or an error for the person.
    Answer(u64, Result<String, String>),
    Opened(String),
    Message(String, Vec<u8>),
    Closed(String, String),
}

/// A message in the text form of the run contract: `key=value` lines, then an empty line
/// and the items.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Reply {
    pub fields: BTreeMap<String, String>,
    pub items: Vec<String>,
}

impl Reply {
    pub fn parse(text: &str) -> Reply {
        let (head, items) = text.split_once("\n\n").unwrap_or((text, ""));
        Reply {
            fields: head
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            items: items.lines().map(str::to_owned).collect(),
        }
    }

    fn get(&self, key: &str) -> &str {
        self.fields.get(key).map_or("", String::as_str)
    }
}

/// What a request is for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Ask {
    Terminals,
    Open,
    Close(String),
    Panes,
    PaneChange,
}

/// The pane backend, from `GET /api/v1/panes`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Backend {
    pub kind: String,
    pub session: String,
    pub running: bool,
    pub detail: String,
}

/// One pane, from an item `<pane> <state> <terminal or -> <title>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneRow {
    pub id: String,
    pub state: String,
    pub terminal: Option<String>,
    pub title: String,
}

/// The time between 2 reads of the terminal list and of the panes. Each read of the
/// herdr panes runs 3 herdr commands.
const REFRESH_SECONDS: f64 = 5.0;

pub struct Sidebar {
    remote: Box<dyn Remote>,
    next: u64,
    asks: BTreeMap<u64, Ask>,
    pub terms: Vec<Term>,
    pub active: usize,
    /// The herdr session of the daemon, or `None` for the pty backend.
    pub herdr: Option<String>,
    pub backend: Option<Backend>,
    pub panes: Vec<PaneRow>,
    pub error: Option<String>,
    pane_title: String,
    pane_command: String,
    refreshed: f64,
}

/// A field value for a request body: one line.
fn field(key: &str, value: &str) -> String {
    format!("{key}={}\n", value.replace(char::is_control, " "))
}

impl Sidebar {
    pub fn new(remote: Box<dyn Remote>) -> Sidebar {
        let mut s = Sidebar {
            remote,
            next: 0,
            asks: BTreeMap::new(),
            terms: Vec::new(),
            active: 0,
            herdr: None,
            backend: None,
            panes: Vec::new(),
            error: None,
            pane_title: String::new(),
            pane_command: String::new(),
            refreshed: 0.0,
        };
        s.refresh();
        s
    }

    fn ask(&mut self, ask: Ask, method: &str, path: &str, body: &str) {
        self.next += 1;
        self.asks.insert(self.next, ask);
        self.remote.request(self.next, method, path, body);
    }

    /// Read the terminal list and the panes again.
    pub fn refresh(&mut self) {
        if !self.asks.values().any(|a| *a == Ask::Terminals) {
            self.ask(Ask::Terminals, "GET", "/api/v1/terminals", "");
        }
        if !self.asks.values().any(|a| *a == Ask::Panes) {
            self.ask(Ask::Panes, "GET", "/api/v1/panes", "");
        }
    }

    /// Open a terminal of `kind` (`shell` or `herdr`) and show it.
    pub fn open(&mut self, kind: &str) {
        self.ask(Ask::Open, "POST", "/api/v1/terminals", &field("kind", kind));
    }

    /// Close terminal `id`: the daemon stops its program.
    pub fn close(&mut self, id: &str) {
        let path = format!("/api/v1/terminals/{id}/close");
        self.ask(Ask::Close(id.into()), "POST", &path, "");
    }

    pub fn open_pane(&mut self, title: &str, command: &str) {
        let mut body = field("title", title);
        if !command.trim().is_empty() {
            body.push_str(&field("command", command));
        }
        self.ask(Ask::PaneChange, "POST", "/api/v1/panes", &body);
    }

    pub fn close_pane(&mut self, id: &str) {
        let path = format!("/api/v1/panes/{id}/close");
        self.ask(Ask::PaneChange, "POST", &path, "");
    }

    /// Show terminal `id` in a tab, and connect to it when it is new.
    pub fn show(&mut self, id: &str, kind: &str) {
        match self.terms.iter().position(|t| t.id == id) {
            Some(i) => self.active = i,
            None => {
                self.terms.push(Term::new(id, kind));
                self.remote.connect(id);
                self.active = self.terms.len() - 1;
            }
        }
    }

    fn term(&mut self, id: &str) -> Option<&mut Term> {
        self.terms.iter_mut().find(|t| t.id == id)
    }

    /// Handle the events of the remote. Returns true when something changed.
    pub fn poll(&mut self) -> bool {
        let events = self.remote.poll();
        let changed = !events.is_empty();
        for event in events {
            match event {
                RemoteEvent::Answer(id, answer) => {
                    if let Some(ask) = self.asks.remove(&id) {
                        self.answer(ask, answer);
                    }
                }
                RemoteEvent::Opened(t) => {
                    if let Some(term) = self.term(&t) {
                        term.opened();
                    }
                }
                RemoteEvent::Message(t, bytes) => {
                    let Some(term) = self.term(&t) else { continue };
                    if let Err(e) = term.receive(&bytes) {
                        term.connection = Connection::Closed(format!("a bad message: {e}"));
                        self.remote.disconnect(&t);
                    }
                }
                RemoteEvent::Closed(t, why) => {
                    if let Some(term) = self.term(&t) {
                        term.connection = Connection::Closed(why);
                    }
                }
            }
        }
        changed
    }

    fn answer(&mut self, ask: Ask, answer: Result<String, String>) {
        let reply = match answer {
            Ok(text) => Reply::parse(&text),
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        match ask {
            Ask::Terminals => {
                self.herdr = Some(reply.get("herdr"))
                    .filter(|h| *h != "-" && !h.is_empty())
                    .map(str::to_owned);
                let active = self.terms.get(self.active).map(|t| t.id.clone());
                for item in &reply.items {
                    let mut words = item.splitn(4, ' ');
                    let (Some(id), Some(kind)) = (words.next(), words.next()) else {
                        continue;
                    };
                    // A PTY pane shows in a tab when the person asks for it.
                    if kind != "pane" && !self.terms.iter().any(|t| t.id == id) {
                        self.terms.push(Term::new(id, kind));
                        self.remote.connect(id);
                    }
                }
                // A terminal that another page closed has gone from the daemon.
                let known: Vec<&str> = reply
                    .items
                    .iter()
                    .filter_map(|i| i.split(' ').next())
                    .collect();
                for t in self
                    .terms
                    .iter()
                    .filter(|t| !known.contains(&t.id.as_str()))
                {
                    self.remote.disconnect(&t.id);
                }
                self.terms.retain(|t| known.contains(&t.id.as_str()));
                self.active = active
                    .and_then(|a| self.terms.iter().position(|t| t.id == a))
                    .unwrap_or(0);
            }
            Ask::Open => {
                let id = reply.get("terminal").to_owned();
                let kind = reply.get("kind").to_owned();
                self.show(&id, &kind);
            }
            Ask::Close(id) => {
                self.remote.disconnect(&id);
                self.terms.retain(|t| t.id != id);
                self.active = self.active.min(self.terms.len().saturating_sub(1));
                self.refresh();
            }
            Ask::Panes => {
                self.backend = Some(Backend {
                    kind: reply.get("backend").into(),
                    session: reply.get("session").into(),
                    running: reply.get("running") == "yes",
                    detail: reply.get("detail").into(),
                });
                self.panes = reply
                    .items
                    .iter()
                    .filter_map(|i| {
                        let mut w = i.splitn(4, ' ');
                        let (id, state, t) = (w.next()?, w.next()?, w.next()?);
                        Some(PaneRow {
                            id: id.into(),
                            state: state.into(),
                            terminal: (t != "-").then(|| t.to_owned()),
                            title: w.next().unwrap_or("").into(),
                        })
                    })
                    .collect();
            }
            Ask::PaneChange => self.refresh(),
        }
    }

    /// Send the messages of each terminal to the daemon.
    fn flush(&mut self) {
        for t in &mut self.terms {
            for m in t.out.drain(..) {
                if t.connection == Connection::Open {
                    self.remote.send(&t.id, &m);
                }
            }
        }
    }

    /// The label of a tab: the terminal ID first, because 2 shells can have the same title.
    fn label(t: &Term) -> String {
        let name: String = match (t.kind.as_str(), t.title.is_empty()) {
            ("herdr", _) => "Herdr".to_owned(),
            (kind, true) => kind.to_owned(),
            (_, false) => t.title.chars().take(24).collect(),
        };
        match t.exit {
            Some(code) => format!("{} {name} (exit {code})", t.id),
            None => format!("{} {name}", t.id),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        if now - self.refreshed > REFRESH_SECONDS {
            self.refreshed = now;
            self.refresh();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));
        ui.horizontal_wrapped(|ui| {
            ui.strong("Terminal");
            if ui
                .button("+ Shell")
                .on_hover_text("open a new shell on this machine")
                .clicked()
            {
                self.open("shell");
            }
            let herdr = ui.add_enabled(self.herdr.is_some(), egui::Button::new("Herdr"));
            let hint = match &self.herdr {
                Some(s) => format!("open Herdr on the herdr session {s}"),
                None => "the pane backend is pty: start fh serve with herdr installed".into(),
            };
            if herdr
                .on_hover_text(hint)
                .on_disabled_hover_text("the pane backend is pty")
                .clicked()
            {
                self.open("herdr");
            }
        });
        if let Some(e) = self.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(ui.visuals().error_fg_color, e);
                if ui
                    .small_button("x")
                    .on_hover_text("hide this message")
                    .clicked()
                {
                    self.error = None;
                }
            });
        }
        let mut close = None;
        ui.horizontal_wrapped(|ui| {
            for (i, t) in self.terms.iter().enumerate() {
                if ui
                    .selectable_label(i == self.active, Sidebar::label(t))
                    .clicked()
                {
                    self.active = i;
                }
            }
            if let Some(t) = self.terms.get(self.active) {
                let tip = "close this terminal: its program stops";
                if ui.small_button("Close").on_hover_text(tip).clicked() {
                    close = Some(t.id.clone());
                }
            }
        });
        if let Some(id) = close {
            self.close(&id);
        }
        egui::Panel::bottom("panes")
            .resizable(false)
            .show(ui, |ui| self.panes_ui(ui));
        let reconnect = match self.terms.get_mut(self.active) {
            None => {
                ui.weak("No terminal is open. Use + Shell to open one.");
                false
            }
            Some(t) => {
                let mut again = false;
                if let Connection::Closed(why) = &t.connection {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("Not connected: {why}"),
                        );
                        again = t.exit.is_none() && ui.button("Connect again").clicked();
                    });
                }
                if let Some(code) = t.exit {
                    ui.label(format!("The program ended with exit code {code}."));
                }
                t.ui(ui);
                again
            }
        };
        if reconnect {
            let id = self.terms[self.active].id.clone();
            self.terms[self.active].connection = Connection::Connecting;
            self.remote.connect(&id);
        }
        self.flush();
    }

    fn panes_ui(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Panes")
            .default_open(true)
            .show(ui, |ui| {
                let Some(b) = self.backend.clone() else {
                    ui.weak("Reading the pane backend...");
                    return;
                };
                let line = match (b.kind.as_str(), b.running) {
                    ("herdr", true) => {
                        format!("Backend: herdr, session {} ({})", b.session, b.detail)
                    }
                    ("herdr", false) => format!(
                        "Backend: herdr, session {} is not running ({}). Use Herdr to start it.",
                        b.session, b.detail
                    ),
                    _ => format!("Backend: {} ({})", b.kind, b.detail),
                };
                ui.label(line);
                let mut show = None;
                let mut close = None;
                for p in &self.panes {
                    ui.horizontal(|ui| {
                        ui.monospace(&p.id);
                        ui.label(format!("{} ({})", p.title, p.state));
                        if let Some(t) = &p.terminal {
                            if ui.small_button("Show").clicked() {
                                show = Some(t.clone());
                            }
                        }
                        if ui.small_button("Close").clicked() {
                            close = Some(p.id.clone());
                        }
                    });
                }
                if b.running && self.panes.is_empty() {
                    ui.weak("No pane is open.");
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.pane_title)
                            .hint_text("title")
                            .desired_width(90.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.pane_command)
                            .hint_text("command (optional)")
                            .desired_width(170.0),
                    );
                    let title = self.pane_title.trim().to_owned();
                    if ui
                        .add_enabled(
                            b.running && !title.is_empty(),
                            egui::Button::new("New pane"),
                        )
                        .clicked()
                    {
                        let command = self.pane_command.clone();
                        self.open_pane(&title, &command);
                        self.pane_title.clear();
                        self.pane_command.clear();
                    }
                });
                if let Some(t) = show {
                    self.show(&t, "pane");
                }
                if let Some(p) = close {
                    self.close_pane(&p);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A fake daemon: it records each call, and the test gives the events.
    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        events: Vec<RemoteEvent>,
    }

    struct Fake(Rc<RefCell<Log>>);

    impl Remote for Fake {
        fn request(&mut self, id: u64, method: &str, path: &str, body: &str) {
            let call = format!(
                "{id} {method} {path} {}",
                body.trim_end().replace('\n', ";")
            );
            self.0.borrow_mut().calls.push(call.trim_end().to_owned());
        }
        fn connect(&mut self, t: &str) {
            self.0.borrow_mut().calls.push(format!("connect {t}"));
        }
        fn send(&mut self, t: &str, m: &[u8]) {
            self.0.borrow_mut().calls.push(format!("send {t} {m:?}"));
        }
        fn disconnect(&mut self, t: &str) {
            self.0.borrow_mut().calls.push(format!("disconnect {t}"));
        }
        fn poll(&mut self) -> Vec<RemoteEvent> {
            std::mem::take(&mut self.0.borrow_mut().events)
        }
    }

    fn sidebar() -> (Sidebar, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        (Sidebar::new(Box::new(Fake(log.clone()))), log)
    }

    fn calls(log: &Rc<RefCell<Log>>) -> Vec<String> {
        std::mem::take(&mut log.borrow_mut().calls)
    }

    fn give(s: &mut Sidebar, log: &Rc<RefCell<Log>>, events: Vec<RemoteEvent>) {
        log.borrow_mut().events = events;
        s.poll();
    }

    fn answer(id: u64, text: &str) -> RemoteEvent {
        RemoteEvent::Answer(id, Ok(text.into()))
    }

    #[test]
    fn a_tab_label_starts_with_the_terminal() {
        let mut t = Term::new("t2", "shell");
        assert_eq!(Sidebar::label(&t), "t2 shell");
        t.title = "vim notes.txt with a long name".into();
        t.exit = Some(1);
        assert_eq!(Sidebar::label(&t), "t2 vim notes.txt with a lon (exit 1)");
        assert_eq!(Sidebar::label(&Term::new("t3", "herdr")), "t3 Herdr");
    }

    #[test]
    fn replies_read_fields_and_items() {
        let r = Reply::parse("a=1\nb=x=y\n\nt1 shell running my title\nt2 herdr exited\n");
        assert_eq!((r.get("a"), r.get("b"), r.get("c")), ("1", "x=y", ""));
        assert_eq!(r.items, ["t1 shell running my title", "t2 herdr exited"]);
        assert_eq!(Reply::parse("a=1\n").items, Vec::<String>::new());
    }

    #[test]
    fn the_sidebar_shows_the_terminals_of_the_daemon_and_opens_new_ones() {
        let (mut s, log) = sidebar();
        assert_eq!(
            calls(&log),
            ["1 GET /api/v1/terminals", "2 GET /api/v1/panes"]
        );
        // A new page shows the terminals that run. A PTY pane waits for Show.
        let list = "contract=1\nherdr=-\npanes=pty\n\nt1 shell running \nt2 pane running build\n";
        let panes = "backend=pty\ndetail=plain\nrunning=yes\nsession=-\n\nt2 running t2 build\n";
        give(&mut s, &log, vec![answer(1, list), answer(2, panes)]);
        assert_eq!(calls(&log), ["connect t1"]);
        assert_eq!(
            s.terms.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t1"]
        );
        assert_eq!(s.herdr, None);
        let b = s.backend.clone().unwrap();
        assert_eq!((b.kind.as_str(), b.running), ("pty", true));
        let row = PaneRow {
            id: "t2".into(),
            state: "running".into(),
            terminal: Some("t2".into()),
            title: "build".into(),
        };
        assert_eq!(s.panes, [row]);
        // Open a shell, then show the pane, then a second Show of the same terminal.
        s.open("shell");
        give(
            &mut s,
            &log,
            vec![answer(3, "kind=shell\nstate=running\nterminal=t3\n")],
        );
        assert_eq!(
            calls(&log),
            ["3 POST /api/v1/terminals kind=shell", "connect t3"]
        );
        assert_eq!((s.terms.len(), s.active), (2, 1));
        s.show("t2", "pane");
        s.show("t3", "shell");
        assert_eq!(calls(&log), ["connect t2"]);
        assert_eq!(s.active, 1);
        // Messages go only on an open WebSocket.
        s.terms[1].out.push(vec![1, b'x']);
        s.flush();
        give(&mut s, &log, vec![RemoteEvent::Opened("t3".into())]);
        s.terms[1].out.push(vec![1, b'y']);
        s.flush();
        assert_eq!(calls(&log), ["send t3 [1, 121]"]);
        // A bad message closes the connection with a reason.
        give(
            &mut s,
            &log,
            vec![RemoteEvent::Message("t3".into(), vec![9])],
        );
        assert_eq!(calls(&log), ["disconnect t3"]);
        assert!(
            matches!(&s.terms[1].connection, Connection::Closed(w) if w.contains("unknown message"))
        );
        // Close asks the daemon, and the tab goes when the daemon answers.
        s.close("t3");
        give(&mut s, &log, vec![answer(4, "terminal=t3\nstate=closed\n")]);
        let c = calls(&log);
        assert_eq!(
            c[..2],
            ["4 POST /api/v1/terminals/t3/close", "disconnect t3"]
        );
        assert_eq!(
            s.terms.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t1", "t2"]
        );
        // A terminal that another page closed goes at the next list.
        give(
            &mut s,
            &log,
            vec![answer(5, "herdr=femahoy\n\nt1 shell running \n")],
        );
        assert_eq!(
            s.terms.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["t1"]
        );
        assert_eq!(calls(&log), ["disconnect t2"]);
        assert_eq!(s.herdr.as_deref(), Some("femahoy"));
    }

    #[test]
    fn panes_and_errors_go_through_the_daemon() {
        let (mut s, log) = sidebar();
        calls(&log);
        s.open_pane("build", "make -j4\nrm -rf /");
        s.close_pane("w1:p2");
        s.open_pane("lab", "  ");
        assert_eq!(
            calls(&log),
            [
                "3 POST /api/v1/panes title=build;command=make -j4 rm -rf /",
                "4 POST /api/v1/panes/w1:p2/close",
                "5 POST /api/v1/panes title=lab",
            ]
        );
        // A change reads the panes again. An error shows to the person.
        let error =
            RemoteEvent::Answer(5, Err("HTTP 409: the herdr session is not running".into()));
        give(&mut s, &log, vec![answer(3, "pane=w1:p1\n"), error]);
        assert_eq!(
            calls(&log),
            Vec::<String>::new(),
            "the first reads are still open"
        );
        give(
            &mut s,
            &log,
            vec![
                answer(1, "herdr=femahoy\n"),
                answer(2, "backend=herdr\nrunning=no\n"),
            ],
        );
        give(&mut s, &log, vec![answer(4, "")]);
        assert_eq!(
            calls(&log),
            ["6 GET /api/v1/terminals", "7 GET /api/v1/panes"]
        );
        assert_eq!(
            s.error.as_deref(),
            Some("HTTP 409: the herdr session is not running")
        );
        assert!(!s.backend.clone().unwrap().running);
        assert!(s.panes.is_empty());
    }
}
