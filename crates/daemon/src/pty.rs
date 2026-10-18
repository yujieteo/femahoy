//! The terminals of the daemon (DESIGN.md section 4.3). Each terminal is a PTY with a
//! program in it (a shell or the herdr client) and a terminal emulator that keeps its
//! screen (`alacritty_terminal`). The daemon owns the PTY: a browser that closes stops
//! nothing, and a terminal ends only when its program ends or on a close request.
//!
//! A program in a terminal never gets the `HERDR_*` variables of the daemon, so a herdr
//! client in the sidebar never attaches to the herdr session that started `fh serve`.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config as TermConfig, Osc52, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use crate::contract::Error;
use crate::screen::{self, cursor, mode, style, Screen, Span};

/// The most terminals at one time. An ended terminal makes room for a new one.
pub const MAX_TERMINALS: usize = 16;
/// The lines of scrollback of each terminal.
pub const HISTORY: usize = 10_000;
const START_SIZE: (u16, u16) = (100, 30);
/// The time that a program gets to end after SIGHUP, and again after SIGKILL.
const END_WAIT: Duration = Duration::from_secs(1);

/// What a terminal is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A shell that a person opened in the sidebar.
    Shell,
    /// The herdr client on the herdr session of femahoy.
    Herdr,
    /// A pane of the plain PTY backend (`panes.rs`).
    Pane,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Shell => "shell",
            Kind::Herdr => "herdr",
            Kind::Pane => "pane",
        }
    }
}

/// A terminal to open.
pub struct Spec {
    pub kind: Kind,
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[derive(Default)]
struct Live {
    /// Goes up at each change of the screen, the title or the state.
    version: u64,
    title: String,
    exit: Option<i32>,
    ended: bool,
    closed: bool,
    size: (u16, u16),
    sender: Option<EventLoopSender>,
}

#[derive(Default)]
struct Shared {
    live: Mutex<Live>,
    changed: Condvar,
}

impl Shared {
    fn update(&self, f: impl FnOnce(&mut Live)) {
        let mut live = self.live.lock().unwrap();
        f(&mut live);
        live.version += 1;
        self.changed.notify_all();
    }

    /// Write `bytes` to the PTY. The emulator calls this for its answers.
    fn input(&self, bytes: Vec<u8>) {
        if let Some(sender) = &self.live.lock().unwrap().sender {
            let _ = sender.send(Msg::Input(Cow::Owned(bytes)));
        }
    }
}

/// The events of the emulator. It runs on the thread of the PTY with the terminal locked,
/// so it never locks the terminal.
#[derive(Clone)]
struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                self.0.update(|_| {})
            }
            Event::Title(t) => self.0.update(|l| l.title = clean_title(&t)),
            Event::ResetTitle => self.0.update(|l| l.title.clear()),
            Event::PtyWrite(text) => self.0.input(text.into_bytes()),
            // The answer uses the fixed palette. A colour that a program set with OSC 4
            // is in the terminal, which is locked here.
            Event::ColorRequest(index, format) => {
                self.0.input(format(rgb_of(palette(index))).into_bytes())
            }
            Event::TextAreaSizeRequest(format) => {
                let (cols, rows) = self.0.live.lock().unwrap().size;
                self.0.input(format(window(cols, rows)).into_bytes());
            }
            Event::ChildExit(status) => self.0.update(|l| l.exit = Some(exit_code(status))),
            // OSC 52 is off, and the bell and `Exit` need no action.
            _ => {}
        }
    }
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return -signal;
    }
    status.code().unwrap_or(-1)
}

/// A title for the text form: one line of at most 120 characters.
fn clean_title(t: &str) -> String {
    t.chars().filter(|c| !c.is_control()).take(120).collect()
}

fn size(cols: u16, rows: u16) -> TermSize {
    TermSize::new(cols as usize, rows as usize)
}

fn window(cols: u16, rows: u16) -> WindowSize {
    WindowSize {
        num_cols: cols,
        num_lines: rows,
        cell_width: 8,
        cell_height: 16,
    }
}

/// The record of a terminal for the text form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalInfo {
    pub id: String,
    pub kind: Kind,
    /// `running`, `exited` or `closed`.
    pub state: &'static str,
    pub title: String,
    pub exit: Option<i32>,
}

/// One terminal: a PTY, its program and its screen.
pub struct Terminal {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    term: Arc<FairMutex<Term<Listener>>>,
    shared: Arc<Shared>,
    pid: i32,
    reaper: Mutex<Option<JoinHandle<()>>>,
}

/// The `/usr/bin/env` options that remove each `HERDR_*` variable of `vars`.
pub(crate) fn unset_herdr(vars: impl Iterator<Item = String>) -> Vec<String> {
    vars.filter(|k| k.starts_with("HERDR_"))
        .flat_map(|k| ["-u".to_owned(), k])
        .collect()
}

pub(crate) fn env_names() -> impl Iterator<Item = String> {
    std::env::vars_os().filter_map(|(k, _)| k.into_string().ok())
}

/// The full path of `program`, from `PATH` when it has no `/`.
pub fn find_program(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = Path::new(program);
        return p.is_file().then(|| p.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|p| p.is_file())
}

impl Terminal {
    fn spawn(id: String, spec: Spec) -> Result<Arc<Terminal>, Error> {
        let program = spec.argv.first().map_or("", String::as_str);
        let Some(found) = find_program(program) else {
            let message = format!("the program {program:?} is not on PATH");
            return Err(Error::new(409, "term.program", message));
        };
        let mut argv = spec.argv.clone();
        argv[0] = found.display().to_string();
        let env = [
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            ("TERM_PROGRAM", "femahoy"),
        ];
        let mut options = tty::Options {
            working_directory: spec.cwd.clone(),
            env: env.map(|(k, v)| (k.into(), v.into())).into(),
            ..Default::default()
        };
        options.shell = Some(if cfg!(unix) {
            let mut args = unset_herdr(env_names());
            args.extend(argv);
            tty::Shell::new("/usr/bin/env".into(), args)
        } else {
            tty::Shell::new(argv.remove(0), argv)
        });
        let (cols, rows) = START_SIZE;
        let pty = tty::new(&options, window(cols, rows), 0)
            .map_err(|e| Error::new(500, "term.spawn", format!("{program}: {e}")))?;
        let pid = pty.child().id() as i32;
        let shared = Arc::new(Shared::default());
        let listener = Listener(shared.clone());
        let config = TermConfig {
            scrolling_history: HISTORY,
            kitty_keyboard: false,
            osc52: Osc52::Disabled,
            ..Default::default()
        };
        let term = Term::new(config, &size(cols, rows), listener.clone());
        let term = Arc::new(FairMutex::new(term));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)
            .map_err(|e| Error::new(500, "term.spawn", e.to_string()))?;
        {
            let mut live = shared.live.lock().unwrap();
            live.sender = Some(event_loop.channel());
            live.size = (cols, rows);
        }
        let pty_thread = event_loop.spawn();
        let ended = shared.clone();
        // The loop ends when the program ends. Its drop then sends SIGHUP to the pid and
        // waits, so the drop must come at once: the reaped pid is free for a new process.
        let reaper = std::thread::Builder::new()
            .name(format!("fh-term-{id}"))
            .spawn(move || {
                drop(pty_thread.join());
                ended.update(|l| {
                    l.ended = true;
                    l.sender = None;
                });
            })
            .map_err(|e| Error::new(500, "term.spawn", e.to_string()))?;
        Ok(Arc::new(Terminal {
            id,
            kind: spec.kind,
            name: spec.name,
            term,
            shared,
            pid,
            reaper: Mutex::new(Some(reaper)),
        }))
    }

    pub fn info(&self) -> TerminalInfo {
        let live = self.shared.live.lock().unwrap();
        TerminalInfo {
            id: self.id.clone(),
            kind: self.kind,
            state: [["running", "exited"], ["closed"; 2]][live.closed as usize]
                [live.ended as usize],
            title: match live.title.is_empty() || self.kind == Kind::Pane {
                true => self.name.clone(),
                false => live.title.clone(),
            },
            exit: live.exit,
        }
    }

    pub fn running(&self) -> bool {
        let live = self.shared.live.lock().unwrap();
        !live.ended && !live.closed
    }

    fn send(&self, msg: Msg) -> Result<(), Error> {
        let live = self.shared.live.lock().unwrap();
        let sent = live
            .sender
            .as_ref()
            .filter(|_| !live.closed)
            .map(|s| s.send(msg));
        match sent {
            Some(Ok(())) => Ok(()),
            _ => Err(Error::new(409, "term.ended", "the terminal has ended")),
        }
    }

    /// Write `bytes` to the program. The view goes back to the bottom of the scrollback.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.send(Msg::Input(Cow::Owned(bytes.to_vec())))?;
        let scrolled = {
            let mut term = self.term.lock();
            let scrolled = term.grid().display_offset() != 0;
            term.scroll_display(Scroll::Bottom);
            scrolled
        };
        if scrolled {
            self.shared.update(|_| {});
        }
        Ok(())
    }

    /// Set the size. Each value is clamped to the protocol limits.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), Error> {
        let (cols, rows) = (
            cols.clamp(2, screen::MAX_COLS),
            rows.clamp(1, screen::MAX_ROWS),
        );
        if self.shared.live.lock().unwrap().size == (cols, rows) {
            return Ok(());
        }
        self.send(Msg::Resize(window(cols, rows)))?;
        self.term.lock().resize(size(cols, rows));
        self.shared.update(|l| l.size = (cols, rows));
        Ok(())
    }

    /// Scroll the view `lines` up into the scrollback (down when negative), or to the
    /// bottom when `lines` is 0.
    pub fn scroll(&self, lines: i32) {
        let to = if lines == 0 {
            Scroll::Bottom
        } else {
            Scroll::Delta(lines)
        };
        self.term.lock().scroll_display(to);
        self.shared.update(|_| {});
    }

    /// Wait until the version is not `seen`, the terminal ends, or `timeout`. Returns the
    /// version and whether the terminal has ended.
    pub fn wait(&self, seen: u64, timeout: Duration) -> (u64, bool) {
        let end = Instant::now() + timeout;
        let mut live = self.shared.live.lock().unwrap();
        while live.version == seen && !live.ended && !live.closed {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            live = self.shared.changed.wait_timeout(live, left).unwrap().0;
        }
        (live.version, live.ended || live.closed)
    }

    /// The visible screen, with the colours resolved to RGB.
    pub fn screen(&self) -> Screen {
        let term = self.term.lock();
        let content = term.renderable_content();
        let (cols, rows) = (term.columns(), term.screen_lines());
        let offset = content.display_offset;
        let colors = content.colors;
        let lines = (0..rows)
            .map(|row| row_spans(&term.grid()[Line(row as i32 - offset as i32)], cols, colors))
            .collect();
        let point = content.cursor.point;
        let cursor_row = point.line.0 + offset as i32;
        let shape = match content.cursor.shape {
            _ if cursor_row < 0 || cursor_row >= rows as i32 => cursor::HIDDEN,
            CursorShape::Block => cursor::BLOCK,
            CursorShape::Underline => cursor::UNDERLINE,
            CursorShape::Beam => cursor::BEAM,
            CursorShape::HollowBlock => cursor::HOLLOW,
            CursorShape::Hidden => cursor::HIDDEN,
        };
        Screen {
            cols: cols as u16,
            rows: rows as u16,
            cursor_col: point.column.0 as u16,
            cursor_row: cursor_row.max(0) as u16,
            cursor_shape: shape,
            modes: modes(content.mode),
            fg: resolve(Color::Named(NamedColor::Foreground), colors),
            bg: resolve(Color::Named(NamedColor::Background), colors),
            scrolled: offset as u32,
            history: term.grid().history_size() as u32,
            lines,
        }
    }

    /// The last `lines` lines of the scrollback and the screen as plain text. A line that
    /// wrapped is one line here.
    pub fn text(&self, lines: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let cols = term.columns();
        let mut out: Vec<String> = Vec::new();
        let mut current = String::new();
        let top = -(grid.history_size() as i32);
        for l in top..term.screen_lines() as i32 {
            let row = &grid[Line(l)];
            for c in 0..cols {
                let cell = &row[Column(c)];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    current.push(cell.c);
                    current.extend(cell.zerowidth().into_iter().flatten());
                }
            }
            if !row[Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
                out.push(current.trim_end().to_owned());
                current.clear();
            }
        }
        if !current.is_empty() {
            out.push(current.trim_end().to_owned());
        }
        while out.last().is_some_and(|l| l.is_empty()) {
            out.pop();
        }
        let from = out.len().saturating_sub(lines);
        out[from..].join("\n")
    }

    /// Stop the program: SIGHUP to its process group, then SIGKILL when it does not end.
    pub fn close(&self) {
        self.shared.update(|l| l.closed = true);
        let ended = |t: &Terminal| t.shared.live.lock().unwrap().ended;
        #[cfg(unix)]
        for signal in [libc::SIGHUP, libc::SIGKILL] {
            if ended(self) {
                break;
            }
            // The program leads its own process group (setsid in `tty::new`). The group
            // exists while the program is not reaped, and the reaper sets `ended`.
            unsafe {
                libc::kill(-self.pid, signal);
            }
            let end = Instant::now() + END_WAIT;
            while !ended(self) && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if let Some(sender) = &self.shared.live.lock().unwrap().sender {
            let _ = sender.send(Msg::Shutdown);
        }
        if let Some(reaper) = self.reaper.lock().unwrap().take() {
            let _ = reaper.join();
        }
    }
}

fn modes(m: TermMode) -> u32 {
    [
        (TermMode::APP_CURSOR, mode::APP_CURSOR),
        (TermMode::APP_KEYPAD, mode::APP_KEYPAD),
        (TermMode::BRACKETED_PASTE, mode::BRACKETED_PASTE),
        (TermMode::MOUSE_REPORT_CLICK, mode::MOUSE_CLICK),
        (TermMode::MOUSE_DRAG, mode::MOUSE_DRAG),
        (TermMode::MOUSE_MOTION, mode::MOUSE_MOTION),
        (TermMode::SGR_MOUSE, mode::SGR_MOUSE),
        (TermMode::UTF8_MOUSE, mode::UTF8_MOUSE),
        (TermMode::ALT_SCREEN, mode::ALT_SCREEN),
        (TermMode::ALTERNATE_SCROLL, mode::ALTERNATE_SCROLL),
        (TermMode::FOCUS_IN_OUT, mode::FOCUS_IN_OUT),
        (TermMode::LINE_FEED_NEW_LINE, mode::LINE_FEED_NEW_LINE),
    ]
    .iter()
    .filter(|(t, _)| m.contains(*t))
    .fold(0, |bits, (_, b)| bits | b)
}

/// The spans of one row. Blank cells with the default background need no span.
fn row_spans(row: &alacritty_terminal::grid::Row<Cell>, cols: usize, colors: &Colors) -> Vec<Span> {
    let bg0 = resolve(Color::Named(NamedColor::Background), colors);
    let mut spans: Vec<Span> = Vec::new();
    for c in 0..cols {
        let cell = &row[Column(c)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let (fg, bg, mut st) = cell_style(cell, colors);
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        if wide {
            st |= style::WIDE;
        }
        let mut text = String::new();
        if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            || cell.flags.contains(Flags::HIDDEN)
        {
            text.push(' ');
        } else {
            text.push(cell.c);
            text.extend(cell.zerowidth().into_iter().flatten());
        }
        match spans.last_mut() {
            Some(s)
                if !wide
                    && s.style == st
                    && s.fg == fg
                    && s.bg == bg
                    && s.col + s.cols == c as u16 =>
            {
                s.cols += 1;
                s.text.push_str(&text);
            }
            _ => spans.push(Span {
                col: c as u16,
                cols: if wide { 2 } else { 1 },
                fg,
                bg,
                style: st,
                text,
            }),
        }
    }
    // Remove the blank cells at the end of each span that shows no mark on a blank cell.
    for s in &mut spans {
        if s.bg == bg0 && s.style & (style::UNDERLINE | style::STRIKEOUT) == 0 {
            let kept = s.text.trim_end_matches(' ').len();
            s.cols -= (s.text.len() - kept) as u16;
            s.text.truncate(kept);
        }
    }
    spans.retain(|s| s.cols > 0);
    spans
}

fn cell_style(cell: &Cell, colors: &Colors) -> (u32, u32, u8) {
    let f = cell.flags;
    let mut fg = match cell.fg {
        Color::Named(n) if f.contains(Flags::DIM) => resolve(Color::Named(n.to_dim()), colors),
        c if f.contains(Flags::DIM) => dim(resolve(c, colors)),
        c => resolve(c, colors),
    };
    let mut bg = resolve(cell.bg, colors);
    if f.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if f.contains(Flags::HIDDEN) {
        fg = bg;
    }
    let mut st = 0;
    for (flag, bit) in [
        (Flags::BOLD, style::BOLD),
        (Flags::ITALIC, style::ITALIC),
        (Flags::ALL_UNDERLINES, style::UNDERLINE),
        (Flags::STRIKEOUT, style::STRIKEOUT),
    ] {
        if f.intersects(flag) {
            st |= bit;
        }
    }
    (fg, bg, st)
}

fn rgb_of(c: u32) -> alacritty_terminal::vte::ansi::Rgb {
    alacritty_terminal::vte::ansi::Rgb {
        r: (c >> 16) as u8,
        g: (c >> 8) as u8,
        b: c as u8,
    }
}

fn dim(c: u32) -> u32 {
    let ch = |shift: u32| ((c >> shift & 0xff) * 2 / 3) << shift;
    ch(16) | ch(8) | ch(0)
}

/// A colour as `0x00RRGGBB`: the colour that a program set, else the fixed palette.
fn resolve(color: Color, colors: &Colors) -> u32 {
    let index = match color {
        Color::Spec(c) => return (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32,
        Color::Named(n) => n as usize,
        Color::Indexed(i) => i as usize,
    };
    match colors[index] {
        Some(c) => (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32,
        None => palette(index),
    }
}

/// The fixed palette of femahoy: 16 named colours, the 6x6x6 cube, the grey ramp, then
/// the foreground, the background, the cursor and the dim and bright colours.
fn palette(index: usize) -> u32 {
    const NAMED: [u32; 16] = [
        0x181818, 0xac4242, 0x90a959, 0xf4bf75, 0x6a9fb5, 0xaa759f, 0x75b5aa, 0xd8d8d8, 0x6b6b6b,
        0xc55555, 0xaac474, 0xfeca88, 0x82b8c8, 0xc28cb8, 0x93d3c3, 0xf8f8f8,
    ];
    const FG: u32 = 0xd8d8d8;
    const BG: u32 = 0x181818;
    let level = |v: usize| if v == 0 { 0 } else { 55 + 40 * v as u32 };
    match index {
        0..=15 => NAMED[index],
        16..=231 => {
            let i = index - 16;
            level(i / 36) << 16 | level(i / 6 % 6) << 8 | level(i % 6)
        }
        232..=255 => {
            let g = 8 + 10 * (index - 232) as u32;
            g << 16 | g << 8 | g
        }
        256 | 258 => FG,
        257 => BG,
        259..=266 => dim(NAMED[index - 259]),
        267 => NAMED[15],
        268 => dim(FG),
        _ => FG,
    }
}

/// The terminals of one daemon.
pub struct Terminals {
    list: Mutex<Vec<Arc<Terminal>>>,
    next: Mutex<u64>,
}

impl Default for Terminals {
    fn default() -> Self {
        Terminals {
            list: Mutex::new(Vec::new()),
            next: Mutex::new(1),
        }
    }
}

impl Terminals {
    /// Open a terminal. When the list is full, the oldest ended terminal makes room.
    pub fn open(&self, spec: Spec) -> Result<Arc<Terminal>, Error> {
        let mut list = self.list.lock().unwrap();
        if list.len() >= MAX_TERMINALS {
            match list.iter().position(|t| !t.running()) {
                Some(i) => {
                    list.remove(i).close();
                }
                None => {
                    return Err(Error::new(
                        503,
                        "term.limit",
                        format!("{MAX_TERMINALS} terminals are open. Close one first."),
                    ))
                }
            }
        }
        let id = {
            let mut next = self.next.lock().unwrap();
            *next += 1;
            format!("t{}", *next - 1)
        };
        let terminal = Terminal::spawn(id, spec)?;
        list.push(terminal.clone());
        Ok(terminal)
    }

    pub fn get(&self, id: &str) -> Result<Arc<Terminal>, Error> {
        self.list
            .lock()
            .unwrap()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .ok_or_else(|| Error::new(404, "term.unknown", format!("no terminal {id:?}")))
    }

    /// Close the terminal `id` and remove it from the list.
    pub fn close(&self, id: &str) -> Result<(), Error> {
        let t = self.get(id)?;
        self.list.lock().unwrap().retain(|x| x.id != id);
        t.close();
        Ok(())
    }

    /// The running terminal of `kind`, if there is one.
    pub fn running(&self, kind: Kind) -> Option<Arc<Terminal>> {
        let list = self.list.lock().unwrap();
        list.iter().find(|t| t.kind == kind && t.running()).cloned()
    }

    pub fn list(&self) -> Vec<Arc<Terminal>> {
        self.list.lock().unwrap().clone()
    }

    /// Close every terminal at the same time. The daemon calls this when it stops.
    pub fn close_all(&self) {
        let list = std::mem::take(&mut *self.list.lock().unwrap());
        std::thread::scope(|s| {
            for t in &list {
                s.spawn(|| t.close());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Spec {
        Spec {
            kind: Kind::Shell,
            name: "shell".into(),
            argv: vec!["/bin/sh".into(), "-c".into(), script.into()],
            cwd: None,
        }
    }

    fn wait_for(t: &Terminal, what: impl Fn(&Terminal) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        let mut seen = 0;
        while !what(t) {
            assert!(
                Instant::now() < end,
                "timeout; the text is {:?}",
                t.text(50)
            );
            seen = t.wait(seen, Duration::from_millis(200)).0;
        }
    }

    #[test]
    fn a_program_draws_colours_and_wide_text_and_ends_with_its_code() {
        let terms = Terminals::default();
        let t = terms
            .open(sh(
                "printf 'a\\033[1;31mRED\\033[0m \\347\\225\\214x\\n'; exit 3",
            ))
            .unwrap();
        wait_for(&t, |t| t.info().state == "exited");
        assert_eq!(t.info().exit, Some(3));
        let s = t.screen();
        assert_eq!((s.cols, s.rows), START_SIZE);
        let spans = &s.lines[0];
        let texts: Vec<_> = spans.iter().map(|s| s.text.as_str()).collect();
        // The blank cell at column 4 needs no span.
        assert_eq!(texts, ["a", "RED", "界", "x"], "{spans:?}");
        assert_eq!((spans[1].fg, spans[1].style), (0xac4242, style::BOLD));
        assert_eq!(
            (spans[2].col, spans[2].cols, spans[2].style),
            (5, 2, style::WIDE)
        );
        assert_eq!(spans[3].col, 7);
        assert!(t.write(b"x").is_err());
        terms.close_all();
    }

    #[test]
    fn input_resize_scrollback_and_text() {
        let terms = Terminals::default();
        let t = terms
            .open(sh("stty -echo; while read l; do echo \"got:$l\"; done"))
            .unwrap();
        t.resize(40, 5).unwrap();
        // A resize past the limits is clamped.
        t.resize(9999, 0).unwrap();
        assert_eq!((t.screen().cols, t.screen().rows), (screen::MAX_COLS, 1));
        t.resize(40, 5).unwrap();
        for i in 0..8 {
            t.write(format!("{i}\r").as_bytes()).unwrap();
        }
        wait_for(&t, |t| t.text(1) == "got:7");
        assert_eq!(t.text(3), "got:5\ngot:6\ngot:7");
        assert!(t.screen().history >= 3);
        t.scroll(2);
        assert_eq!(t.screen().scrolled, 2);
        // Input brings the view back to the bottom.
        t.write(b"x\r").unwrap();
        assert_eq!(t.screen().scrolled, 0);
        // A long line wraps on the screen but is one line of text.
        let long = "y".repeat(90);
        t.write(format!("{long}\r").as_bytes()).unwrap();
        wait_for(&t, |t| t.text(1).len() == 94);
        t.close();
        assert_eq!(t.info().state, "closed");
        assert!(t.write(b"x").is_err());
    }

    #[test]
    fn close_stops_a_program_that_ignores_sighup() {
        let terms = Terminals::default();
        let t = terms.open(sh("trap '' HUP; echo ready; sleep 60")).unwrap();
        wait_for(&t, |t| t.text(1) == "ready");
        let start = Instant::now();
        t.close();
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(!t.running());
    }

    #[test]
    fn the_program_gets_no_herdr_variable_and_a_missing_program_fails() {
        let names = ["PATH", "HERDR_PANE_ID", "HERDR_SOCKET_PATH"].map(String::from);
        assert_eq!(
            unset_herdr(names.into_iter()),
            ["-u", "HERDR_PANE_ID", "-u", "HERDR_SOCKET_PATH"]
        );
        // A daemon that runs in a herdr pane has HERDR_* variables. The program has none.
        let terms = Terminals::default();
        let t = terms
            .open(sh("echo \"[$(env | grep -c ^HERDR_)] $TERM\""))
            .unwrap();
        wait_for(&t, |t| !t.running());
        assert_eq!(t.text(1), "[0] xterm-256color");
        let mut spec = sh("");
        spec.argv = vec!["fh-no-such-program".into()];
        let e = terms.open(spec).err().unwrap();
        assert_eq!((e.status, e.code.as_str()), (409, "term.program"));
        assert!(terms.get("t9").is_err());
        assert_eq!(terms.list().len(), 1);
    }
}
