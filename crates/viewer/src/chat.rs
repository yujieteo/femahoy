//! The chat panel of the page of `fh serve` (roadmap item M4, `docs/chat.md`). The person
//! writes a request. The daemon runs the agent, and the panel shows each action of the
//! agent: its text, each `fh` command with its result, and the end with the time and the
//! cost. When a run succeeds during the chat, the panel opens its results in the viewer.
//! The browser holds no API key: the agent runs in the daemon with the login of the person.

use std::collections::BTreeMap;

use eframe::egui;

/// The daemon as the chat sees it. `web.rs` has the browser one, and the tests a fake.
pub trait Remote {
    /// Send an HTTP request. `body` has the fields in the text form of the run contract. The
    /// answer comes back from [`Remote::poll`] with the same `id`.
    fn request(&mut self, id: u64, method: &str, path: &str, body: &str);
    /// The answers since the last call.
    fn poll(&mut self) -> Vec<Answer>;
}

/// The answer to request `id`: the text of the message, or an error for the person.
pub type Answer = (u64, Result<String, String>);

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
}

/// The time between 2 reads of the chat while the agent works, and while it does not.
const POLL_RUNNING: f64 = 1.0;
const POLL_IDLE: f64 = 5.0;
/// A result with more lines shows in a closed section.
const OPEN_LINES: usize = 8;

/// One entry of the chat, from an item `<seq> <kind> <text>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub seq: u64,
    pub kind: String,
    pub text: String,
}

impl Entry {
    fn parse(item: &str) -> Option<Entry> {
        let mut words = item.splitn(3, ' ');
        let seq = words.next()?.parse().ok()?;
        let kind = words.next()?.to_owned();
        Some(Entry {
            seq,
            kind,
            text: unescape(words.next().unwrap_or("")),
        })
    }
}

/// The text of an entry: `\n` is a line break and `\\` is a backslash.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            (c, false) => out.push(c),
        }
    }
    out
}

/// Open the results of a run in the viewer: `(project, run)`.
pub type Open = Box<dyn FnMut(&str, &str)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Read,
    Send,
    Stop,
}

pub struct Chat {
    remote: Box<dyn Remote>,
    open: Open,
    next: u64,
    asks: Vec<(u64, Ask)>,
    pub entries: Vec<Entry>,
    since: u64,
    pub running: bool,
    pub input: String,
    pub error: Option<String>,
    polled: f64,
    /// The entries before this sequence number were there when the page opened. Their
    /// runs do not open again.
    first: Option<u64>,
}

impl Chat {
    pub fn new(remote: Box<dyn Remote>, open: Open) -> Chat {
        let mut c = Chat {
            remote,
            open,
            next: 0,
            asks: Vec::new(),
            entries: Vec::new(),
            since: 0,
            running: false,
            input: String::new(),
            error: None,
            polled: 0.0,
            first: None,
        };
        c.read();
        c
    }

    fn ask(&mut self, ask: Ask, method: &str, path: &str, body: &str) {
        self.next += 1;
        self.asks.push((self.next, ask));
        self.remote.request(self.next, method, path, body);
    }

    fn read(&mut self) {
        if !self.asks.iter().any(|(_, a)| *a == Ask::Read) {
            let path = format!("/api/v1/chat?since={}", self.since);
            self.ask(Ask::Read, "GET", &path, "");
        }
    }

    /// Send the request of the person to the agent.
    pub fn send(&mut self) {
        let text = self.input.trim().to_owned();
        if text.is_empty() || self.running {
            return;
        }
        // A request ends at the end of the line of its field, so a line break is a space.
        let body = format!("text={}\n", text.replace(['\n', '\r'], " "));
        self.ask(Ask::Send, "POST", "/api/v1/chat", &body);
        self.input.clear();
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.ask(Ask::Stop, "POST", "/api/v1/chat/stop", "");
    }

    /// Handle the answers of the daemon.
    pub fn poll(&mut self) {
        for (id, answer) in self.remote.poll() {
            let Some(i) = self.asks.iter().position(|(n, _)| *n == id) else {
                continue;
            };
            let (_, ask) = self.asks.remove(i);
            let reply = match answer {
                Ok(text) => Reply::parse(&text),
                Err(e) => {
                    self.error = Some(e);
                    if ask == Ask::Send {
                        self.running = false;
                    }
                    continue;
                }
            };
            match ask {
                Ask::Read => self.add(&reply),
                Ask::Send | Ask::Stop => {
                    self.error = None;
                    self.read();
                }
            }
        }
    }

    fn add(&mut self, reply: &Reply) {
        let next = reply.fields.get("next").and_then(|n| n.parse().ok());
        let first = *self.first.get_or_insert(next.unwrap_or(0));
        for entry in reply.items.iter().filter_map(|i| Entry::parse(i)) {
            if entry.seq <= self.since {
                continue;
            }
            self.since = entry.seq;
            if entry.kind == "open" && entry.seq > first {
                if let Some((p, r)) = entry.text.split_once(' ') {
                    (self.open)(p, r);
                }
            }
            self.entries.push(entry);
        }
        self.running = reply.fields.get("state").map(String::as_str) == Some("running");
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        let every = if self.running {
            POLL_RUNNING
        } else {
            POLL_IDLE
        };
        if now - self.polled > every {
            self.polled = now;
            self.read();
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(every));
        ui.horizontal(|ui| {
            ui.strong("Chat");
            if self.running {
                ui.spinner();
                ui.weak("the agent works");
            }
        });
        if let Some(e) = self.error.clone() {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        egui::Panel::bottom("chat input")
            .resizable(false)
            .show(ui, |ui| self.input_ui(ui));
        let mut reopen = None;
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.entries.is_empty() {
                    ui.weak(
                        "Ask the agent for a model, for example: build and solve a steel \
                         cantilever of 100 x 10 x 10 mm with 1 MPa on the top face.",
                    );
                }
                for e in &self.entries {
                    if let Some(run) = entry_ui(ui, e) {
                        reopen = Some(run);
                    }
                }
            });
        if let Some((p, r)) = reopen {
            (self.open)(&p, &r);
        }
    }

    fn input_ui(&mut self, ui: &mut egui::Ui) {
        let edit = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .hint_text("a request for the agent (Ctrl+Enter sends)")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let keyed = edit.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && ui.input(|i| i.modifiers.command);
        ui.horizontal(|ui| {
            let ready = !self.running && !self.input.trim().is_empty();
            if ui.add_enabled(ready, egui::Button::new("Send")).clicked() || (keyed && ready) {
                self.send();
            }
            if ui
                .add_enabled(self.running, egui::Button::new("Stop"))
                .on_hover_text("stop the agent and the commands that it started")
                .clicked()
            {
                self.stop();
            }
        });
    }
}

/// Show one entry. Returns the run to show again when the person clicks its button.
fn entry_ui(ui: &mut egui::Ui, e: &Entry) -> Option<(String, String)> {
    let error = ui.visuals().error_fg_color;
    match e.kind.as_str() {
        "you" => {
            ui.separator();
            wrap(ui, egui::RichText::new(format!("You: {}", e.text)).strong());
        }
        "say" => wrap(ui, egui::RichText::new(&e.text)),
        "cmd" => {
            let text = egui::RichText::new(format!("$ {}", e.text))
                .monospace()
                .strong();
            wrap(ui, text);
        }
        "tool" => wrap(ui, egui::RichText::new(&e.text).monospace().weak()),
        "out" | "fail" => {
            let color = (e.kind == "fail").then_some(error);
            let lines: Vec<&str> = e.text.lines().collect();
            let body = |ui: &mut egui::Ui| {
                let text = egui::RichText::new(&e.text).monospace().small();
                wrap(
                    ui,
                    match color {
                        Some(c) => text.color(c),
                        None => text,
                    },
                );
            };
            if lines.len() > OPEN_LINES {
                let first: String = lines[0].chars().take(60).collect();
                let head = format!("{first} ({} lines)", lines.len());
                egui::CollapsingHeader::new(egui::RichText::new(head).monospace().small())
                    .id_salt(e.seq)
                    .show(ui, body);
            } else {
                body(ui);
            }
        }
        "open" => {
            let (p, r) = e.text.split_once(' ')?;
            if ui
                .link(format!("Results of run {r} (project {p})"))
                .clicked()
            {
                return Some((p.to_owned(), r.to_owned()));
            }
        }
        "end" => wrap(
            ui,
            egui::RichText::new(format!("Agent: {}", e.text)).strong(),
        ),
        _ => wrap(ui, egui::RichText::new(&e.text).weak()),
    }
    None
}

/// A label that breaks long lines, so that the panel keeps its width.
fn wrap(ui: &mut egui::Ui, text: egui::RichText) {
    ui.add(egui::Label::new(text).wrap());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Log {
        calls: Vec<String>,
        events: Vec<Answer>,
        opened: Vec<String>,
    }

    struct Fake(Rc<RefCell<Log>>);

    impl Remote for Fake {
        fn request(&mut self, id: u64, method: &str, path: &str, body: &str) {
            let call = format!("{id} {method} {path} {}", body.trim_end());
            self.0.borrow_mut().calls.push(call.trim_end().to_owned());
        }
        fn poll(&mut self) -> Vec<Answer> {
            std::mem::take(&mut self.0.borrow_mut().events)
        }
    }

    fn chat() -> (Chat, Rc<RefCell<Log>>) {
        let log = Rc::new(RefCell::new(Log::default()));
        let opened = log.clone();
        let open = move |p: &str, r: &str| opened.borrow_mut().opened.push(format!("{p} {r}"));
        (Chat::new(Box::new(Fake(log.clone())), Box::new(open)), log)
    }

    fn give(c: &mut Chat, log: &Rc<RefCell<Log>>, id: u64, text: &str) {
        log.borrow_mut().events.push((id, Ok(text.into())));
        c.poll();
    }

    #[test]
    fn replies_read_fields_and_items() {
        let r = Reply::parse("a=1\nb=x=y\n\n1 text hello\n2 end done\n");
        assert_eq!(r.fields.get("a").map(String::as_str), Some("1"));
        assert_eq!(r.fields.get("b").map(String::as_str), Some("x=y"));
        assert_eq!(r.items, ["1 text hello", "2 end done"]);
        assert_eq!(Reply::parse("a=1\n").items, Vec::<String>::new());
    }

    #[test]
    fn entries_keep_line_breaks_and_backslashes() {
        let e = Entry::parse("7 out ok\tgmsh\\nC:\\\\x").unwrap();
        assert_eq!((e.seq, e.kind.as_str()), (7, "out"));
        assert_eq!(e.text, "ok\tgmsh\nC:\\x");
        assert_eq!(Entry::parse("8 note").unwrap().text, "");
        assert_eq!(Entry::parse("x note a"), None);
    }

    #[test]
    fn the_chat_sends_the_request_shows_the_actions_and_opens_new_results() {
        let (mut c, log) = chat();
        // A run of an earlier chat does not open again on a new page.
        give(
            &mut c,
            &log,
            1,
            "next=2\nstate=idle\n\n1 you old\n2 open p1 r1\n",
        );
        assert_eq!(c.entries.len(), 2);
        c.input = "build a cantilever\nand solve it".into();
        c.send();
        assert!(c.running);
        c.send();
        give(&mut c, &log, 2, "state=running\n");
        let stream = "next=6\nstate=running\n\n3 you build a cantilever and solve it\n\
                      4 cmd fh run . --size 2.5\n5 out ok\\tmesh\n6 open p1 r2\n";
        give(&mut c, &log, 3, stream);
        assert_eq!(
            std::mem::take(&mut log.borrow_mut().calls),
            [
                "1 GET /api/v1/chat?since=0",
                "2 POST /api/v1/chat text=build a cantilever and solve it",
                "3 GET /api/v1/chat?since=2",
            ]
        );
        assert_eq!(log.borrow().opened, ["p1 r2"]);
        let kinds: Vec<&str> = c.entries.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["you", "open", "you", "cmd", "out", "open"]);
        // The same entries again change nothing, and the end lets the person send again.
        c.read();
        give(
            &mut c,
            &log,
            4,
            &format!("{stream}7 end done\n").replace("running", "idle"),
        );
        assert_eq!(c.entries.len(), 7);
        assert!(!c.running);
        give(&mut c, &log, 99, "next=9\n\n9 end done\n");
        assert_eq!(c.entries.len(), 7, "an answer to no request");
        // A refused request shows the error and lets the person send again.
        c.input = "again".into();
        c.send();
        log.borrow_mut()
            .events
            .push((5, Err("HTTP 409: busy (chat.busy)".into())));
        c.poll();
        assert!(!c.running);
        assert_eq!(c.error.as_deref(), Some("HTTP 409: busy (chat.busy)"));
    }
}
