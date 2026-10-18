//! The terminal widget of the sidebar. The daemon owns the PTY and the terminal state
//! (`crates/daemon/src/pty.rs`) and sends the screen over a WebSocket in the protocol
//! `fh.term.v1` (`docs/terminal.md`). This module reads the screen messages, draws the
//! screen, and changes the keys, the paste, the wheel and the mouse into messages for the
//! PTY. It has no GPU and no browser types, so its tests run with no window.

use eframe::egui::{self, Color32, FontId, Key, Modifiers, Pos2, Rect, Stroke, StrokeKind};

/// The subprotocol of the terminal WebSocket.
pub const PROTOCOL: &str = "fh.term.v1";
/// The prefix of the subprotocol item that carries the token (a browser cannot set the
/// `Authorization` header of a WebSocket).
pub const TOKEN_PROTOCOL: &str = "fh.token.";

const MSG_SCREEN: u8 = 1;
const MSG_TITLE: u8 = 2;
const MSG_EXIT: u8 = 3;
const MSG_INPUT: u8 = 1;
const MSG_RESIZE: u8 = 2;
const MSG_SCROLL: u8 = 3;
const MAX_COLS: u16 = 500;
const MAX_ROWS: u16 = 200;
const FONT_SIZE: f32 = 13.0;

/// The bits of the `modes` field of a screen message.
pub mod mode {
    pub const APP_CURSOR: u32 = 1;
    pub const BRACKETED_PASTE: u32 = 1 << 2;
    pub const MOUSE_CLICK: u32 = 1 << 3;
    pub const MOUSE_DRAG: u32 = 1 << 4;
    pub const MOUSE_MOTION: u32 = 1 << 5;
    pub const SGR_MOUSE: u32 = 1 << 6;
    pub const UTF8_MOUSE: u32 = 1 << 7;
    pub const ALT_SCREEN: u32 = 1 << 8;
    pub const ALTERNATE_SCROLL: u32 = 1 << 9;
    pub const FOCUS_IN_OUT: u32 = 1 << 10;
    pub const LINE_FEED_NEW_LINE: u32 = 1 << 11;
    /// A program asked for the mouse.
    pub const MOUSE: u32 = MOUSE_CLICK | MOUSE_DRAG | MOUSE_MOTION;
}

/// The bits of the style of a span.
pub mod style {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const STRIKEOUT: u8 = 1 << 3;
    pub const WIDE: u8 = 1 << 4;
}

/// The cursor shapes.
pub mod cursor {
    pub const HIDDEN: u8 = 0;
    pub const BLOCK: u8 = 1;
    pub const UNDERLINE: u8 = 2;
    pub const BEAM: u8 = 3;
}

/// Cells with the same colours and style, from column `col` over `cols` columns.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub col: u16,
    pub cols: u16,
    pub fg: u32,
    pub bg: u32,
    pub style: u8,
    pub text: String,
}

/// The visible screen of a terminal, as the daemon sent it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cursor_shape: u8,
    pub modes: u32,
    pub fg: u32,
    pub bg: u32,
    pub scrolled: u32,
    pub history: u32,
    pub lines: Vec<Vec<Span>>,
}

/// What a message of the daemon changed.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Screen,
    Title(String),
    Exit(i32),
}

struct Reader<'a>(&'a [u8], usize);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self
            .0
            .get(self.1..self.1 + n)
            .ok_or("a short screen message")?;
        self.1 += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
}

impl Screen {
    /// Apply a message of the daemon. A bad message changes nothing.
    pub fn apply(&mut self, message: &[u8]) -> Result<Incoming, String> {
        let (&kind, rest) = message.split_first().ok_or("an empty message")?;
        match (kind, rest.len()) {
            (MSG_TITLE, _) => Ok(Incoming::Title(String::from_utf8_lossy(rest).into_owned())),
            (MSG_EXIT, 4) => Ok(Incoming::Exit(i32::from_be_bytes([
                rest[0], rest[1], rest[2], rest[3],
            ]))),
            (MSG_SCREEN, _) => {
                *self = self.next(rest)?;
                Ok(Incoming::Screen)
            }
            (k, n) => Err(format!("an unknown message type {k} with {n} bytes")),
        }
    }

    fn next(&self, bytes: &[u8]) -> Result<Screen, String> {
        let mut r = Reader(bytes, 0);
        let (cols, rows) = (r.u16()?, r.u16()?);
        if !(1..=MAX_COLS).contains(&cols) || !(1..=MAX_ROWS).contains(&rows) {
            return Err(format!("a bad screen size {cols}x{rows}"));
        }
        let same = cols == self.cols && rows == self.rows;
        let mut s = Screen {
            cols,
            rows,
            cursor_col: r.u16()?,
            cursor_row: r.u16()?,
            cursor_shape: r.u8()?,
            modes: r.u32()?,
            fg: r.u32()?,
            bg: r.u32()?,
            scrolled: r.u32()?,
            history: r.u32()?,
            lines: match same {
                true => self.lines.clone(),
                false => vec![Vec::new(); rows as usize],
            },
        };
        for _ in 0..r.u16()? {
            let row = r.u16()?;
            if row >= rows {
                return Err(format!("row {row} is not on a screen of {rows} rows"));
            }
            let mut spans = Vec::new();
            for _ in 0..r.u16()? {
                let (col, n, fg, bg, style) = (r.u16()?, r.u16()?, r.u32()?, r.u32()?, r.u8()?);
                let len = r.u16()? as usize;
                let text =
                    std::str::from_utf8(r.take(len)?).map_err(|_| "text that is not UTF-8")?;
                if n == 0 || col as u32 + n as u32 > cols as u32 {
                    return Err(format!(
                        "a span over columns {col}..{} of {cols}",
                        col as u32 + n as u32
                    ));
                }
                let text = text.to_owned();
                spans.push(Span {
                    col,
                    cols: n,
                    fg,
                    bg,
                    style,
                    text,
                });
            }
            s.lines[row as usize] = spans;
        }
        match r.1 == bytes.len() {
            true => Ok(s),
            false => Err("bytes after the last row".into()),
        }
    }
}

pub fn input_message(bytes: &[u8]) -> Vec<u8> {
    [&[MSG_INPUT], bytes].concat()
}

pub fn resize_message(cols: u16, rows: u16) -> Vec<u8> {
    [&[MSG_RESIZE][..], &cols.to_be_bytes(), &rows.to_be_bytes()].concat()
}

/// Move the view `lines` up into the scrollback (down when negative, to the bottom at 0).
pub fn scroll_message(lines: i32) -> Vec<u8> {
    [&[MSG_SCROLL][..], &lines.to_be_bytes()].concat()
}

const LETTERS: [Key; 26] = {
    use Key::*;
    [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ]
};

/// The control character of Control and `key`.
fn control(key: Key) -> Option<u8> {
    if let Some(i) = LETTERS.iter().position(|&k| k == key) {
        return Some(i as u8 + 1);
    }
    Some(match key {
        Key::Space | Key::Num2 => 0,
        Key::OpenBracket => 0x1b,
        Key::Backslash => 0x1c,
        Key::CloseBracket => 0x1d,
        Key::Num6 => 0x1e,
        Key::Minus | Key::Slash => 0x1f,
        _ => return None,
    })
}

/// The bytes of a key press as xterm sends them, or `None` when the key is text (a `Text`
/// event brings it) or a shortcut of the browser: Command on macOS, Control with Shift and a
/// letter, and Control+V on the other systems, which pastes. On macOS, Control+V is a key.
pub fn key_bytes(key: Key, m: Modifiers, modes: u32, mac: bool) -> Option<Vec<u8>> {
    let letter = LETTERS.contains(&key);
    if m.mac_cmd || (m.ctrl && m.shift && letter) || (!mac && m.ctrl && key == Key::V) {
        return None;
    }
    // The xterm modifier parameter: 1 + Shift + 2 Alt + 4 Control.
    let p = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl);
    let app = modes & mode::APP_CURSOR != 0;
    let csi = |c: char| match (p, app) {
        (1, false) => format!("\x1b[{c}"),
        (1, true) => format!("\x1bO{c}"),
        _ => format!("\x1b[1;{p}{c}"),
    };
    let ss3 = |c: char| match p {
        1 => format!("\x1bO{c}"),
        _ => format!("\x1b[1;{p}{c}"),
    };
    let tilde = |n: u8| match p {
        1 => format!("\x1b[{n}~"),
        _ => format!("\x1b[{n};{p}~"),
    };
    let esc = if m.alt { "\x1b" } else { "" };
    let text = match key {
        Key::ArrowUp => csi('A'),
        Key::ArrowDown => csi('B'),
        Key::ArrowRight => csi('C'),
        Key::ArrowLeft => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => ss3('P'),
        Key::F2 => ss3('Q'),
        Key::F3 => ss3('R'),
        Key::F4 => ss3('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        Key::Enter if modes & mode::LINE_FEED_NEW_LINE != 0 => format!("{esc}\r\n"),
        Key::Enter => format!("{esc}\r"),
        Key::Tab if m.shift => "\x1b[Z".into(),
        Key::Tab => format!("{esc}\t"),
        Key::Backspace if m.ctrl => format!("{esc}\x08"),
        Key::Backspace => format!("{esc}\x7f"),
        Key::Escape => "\x1b".into(),
        k if m.ctrl => format!("{esc}{}", control(k)? as char),
        _ => return None,
    };
    Some(text.into_bytes())
}

/// The bytes of a paste. A new line is Enter. In bracketed paste mode the text goes between
/// the paste brackets, and the text has no Escape, so it cannot close the bracket early.
pub fn paste_bytes(text: &str, modes: u32) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    match modes & mode::BRACKETED_PASTE {
        0 => text.into_bytes(),
        _ => format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes(),
    }
}

/// A cell of the visible screen. The order is the reading order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cell {
    pub row: u16,
    pub col: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Drag,
    Move,
}

/// The report of a mouse action for a program that asked for the mouse, or `None`.
/// `button` is 0 (left), 1 (middle), 2 (right), 64 (wheel up) or 65 (wheel down).
pub fn mouse_bytes(
    button: u8,
    at: Cell,
    action: MouseAction,
    m: Modifiers,
    modes: u32,
) -> Option<Vec<u8>> {
    let wanted = match action {
        MouseAction::Press | MouseAction::Release => mode::MOUSE,
        MouseAction::Drag => mode::MOUSE_DRAG | mode::MOUSE_MOTION,
        MouseAction::Move => mode::MOUSE_MOTION,
    };
    if modes & wanted == 0 {
        return None;
    }
    let mods = 4 * u8::from(m.shift) + 8 * u8::from(m.alt) + 16 * u8::from(m.ctrl);
    let sgr = modes & mode::SGR_MOUSE != 0;
    let code = mods
        + match action {
            MouseAction::Press => button,
            // Only the SGR form names the button of a release.
            MouseAction::Release if sgr => button,
            MouseAction::Release => 3,
            MouseAction::Drag => button + 32,
            MouseAction::Move => 3 + 32,
        };
    let (x, y) = (u32::from(at.col) + 1, u32::from(at.row) + 1);
    if sgr {
        let end = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }
    let pos = |v: u32| -> Option<Vec<u8>> {
        match modes & mode::UTF8_MOUSE {
            0 => (v + 32 <= 255).then(|| vec![(v + 32) as u8]),
            _ => char::from_u32(v + 32)
                .filter(|_| v + 32 < 2048)
                .map(|c| c.to_string().into_bytes()),
        }
    };
    Some([&b"\x1b[M"[..], &[32 + code], &pos(x)?, &pos(y)?].concat())
}

/// A span of one cell, or a wide span, is one cluster in its first cell: the daemon sends a
/// cell with zero-width marks as its own span. Other spans have one character for each cell.
fn one_cell(s: &Span) -> bool {
    s.cols == 1 || s.style & style::WIDE != 0
}

/// The text of each cell of a row. A wide character, or text with combining marks, is in its
/// first cell, and the other cells of its span are empty.
fn row_cells(screen: &Screen, row: usize) -> Vec<String> {
    let mut cells = vec![" ".to_owned(); screen.cols as usize];
    for s in screen.lines.get(row).into_iter().flatten() {
        let (start, n) = (s.col as usize, s.cols as usize);
        let chars: Vec<char> = s.text.chars().collect();
        if !one_cell(s) && chars.len() == n {
            for (i, c) in chars.iter().enumerate() {
                cells[start + i] = c.to_string();
            }
        } else {
            cells[start] = s.text.clone();
            cells[start + 1..start + n]
                .iter_mut()
                .for_each(String::clear);
        }
    }
    cells
}

/// The text from cell `a` to cell `b` (both included), one line for each row, with no
/// spaces at the end of a line.
pub fn selected_text(screen: &Screen, a: Cell, b: Cell) -> String {
    let (start, end) = (a.min(b), a.max(b));
    let last = end.row.min(screen.rows.saturating_sub(1));
    let lines: Vec<String> = (start.row..=last)
        .map(|row| {
            let cells = row_cells(screen, row as usize);
            let from = if row == start.row {
                start.col as usize
            } else {
                0
            };
            let to = if row == end.row {
                end.col as usize + 1
            } else {
                cells.len()
            };
            let line = cells.get(from.min(cells.len())..to.min(cells.len()));
            line.unwrap_or_default().concat().trim_end().to_owned()
        })
        .collect();
    lines.join("\n")
}

/// The state of the WebSocket of a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Open,
    Closed(String),
}

/// One terminal of the daemon in the sidebar.
pub struct Term {
    pub id: String,
    pub kind: String,
    /// The title that the program set, or empty.
    pub title: String,
    pub screen: Screen,
    pub exit: Option<i32>,
    pub connection: Connection,
    /// The messages for the daemon. The sidebar sends them after each frame.
    pub out: Vec<Vec<u8>>,
    sent_size: Option<(u16, u16)>,
    selection: Option<(Cell, Cell)>,
    selecting: bool,
    button: Option<u8>,
    last: Option<Cell>,
    wheel: f32,
    focused: bool,
}

fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn draw_text(p: &egui::Painter, pos: Pos2, text: &str, font: &FontId, color: Color32, st: u8) {
    let format = egui::TextFormat {
        font_id: font.clone(),
        color,
        italics: st & style::ITALIC != 0,
        ..Default::default()
    };
    let galley = p.layout_job(egui::text::LayoutJob::single_section(
        text.to_owned(),
        format,
    ));
    if st & style::BOLD != 0 {
        p.galley(pos + egui::vec2(0.6, 0.0), galley.clone(), color);
    }
    p.galley(pos, galley, color);
}

impl Term {
    pub fn new(id: &str, kind: &str) -> Term {
        Term {
            id: id.into(),
            kind: kind.into(),
            title: String::new(),
            screen: Screen::default(),
            exit: None,
            connection: Connection::Connecting,
            out: Vec::new(),
            sent_size: None,
            selection: None,
            selecting: false,
            button: None,
            last: None,
            wheel: 0.0,
            focused: false,
        }
    }

    /// The WebSocket is open. The next frame sends the size of the widget.
    pub fn opened(&mut self) {
        self.connection = Connection::Open;
        self.sent_size = None;
    }

    /// Apply a message of the daemon.
    pub fn receive(&mut self, message: &[u8]) -> Result<(), String> {
        match self.screen.apply(message)? {
            Incoming::Screen => {}
            Incoming::Title(t) => self.title = t,
            Incoming::Exit(code) => self.exit = Some(code),
        }
        Ok(())
    }

    fn report(&mut self, bytes: &[u8]) -> bool {
        let live = !bytes.is_empty() && self.connection == Connection::Open && self.exit.is_none();
        if live {
            self.out.push(input_message(bytes));
        }
        live
    }

    fn type_bytes(&mut self, bytes: &[u8]) {
        if self.report(bytes) {
            self.selection = None;
        }
    }

    /// Draw the terminal in all of the space of `ui`, and read the keys and the mouse.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let font = FontId::monospace(FONT_SIZE);
        let (cw, ch) = ui
            .ctx()
            .fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let size = ui.available_size();
        let want = (
            ((size.x / cw) as u16).clamp(2, MAX_COLS),
            ((size.y / ch) as u16).clamp(1, MAX_ROWS),
        );
        if self.connection == Connection::Open && self.sent_size != Some(want) {
            self.out.push(resize_message(want.0, want.1));
            self.sent_size = Some(want);
        }
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let id = egui::Id::new(("fh-terminal", &self.id));
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        let response = response.on_hover_cursor(egui::CursorIcon::Text);
        self.input(ui, rect, &response, (cw, ch));
        self.paint(ui, rect, &font, (cw, ch));
    }

    fn input(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        response: &egui::Response,
        (cw, ch): (f32, f32),
    ) {
        let focused = response.has_focus();
        if focused {
            let filter = egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            };
            ui.memory_mut(|m| m.set_focus_lock_filter(response.id, filter));
        }
        let modes = self.screen.modes;
        if focused != self.focused {
            self.focused = focused;
            if modes & mode::FOCUS_IN_OUT != 0 {
                self.report(if focused { b"\x1b[I" } else { b"\x1b[O" });
            }
        }
        let (sc, sr) = (self.screen.cols.max(1) - 1, self.screen.rows.max(1) - 1);
        let at = |p: Pos2| Cell {
            col: ((p.x - rect.left()) / cw).clamp(0.0, sc as f32) as u16,
            row: ((p.y - rect.top()) / ch).clamp(0.0, sr as f32) as u16,
        };
        let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
        let (events, now) = ui.input(|i| (i.events.clone(), i.modifiers));
        let mut send: Vec<Vec<u8>> = Vec::new();
        for event in &events {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let held = self.button.is_some() || self.selecting;
                    if (*pressed && !rect.contains(*pos)) || (!*pressed && !held) {
                        continue;
                    }
                    if *pressed {
                        response.request_focus();
                    }
                    let b = match button {
                        egui::PointerButton::Primary => 0,
                        egui::PointerButton::Middle => 1,
                        egui::PointerButton::Secondary => 2,
                        _ => continue,
                    };
                    let cell = at(*pos);
                    // Shift with the mouse selects text, also for a program that has the mouse.
                    if modes & mode::MOUSE != 0 && !modifiers.shift {
                        let action = if *pressed {
                            MouseAction::Press
                        } else {
                            MouseAction::Release
                        };
                        send.extend(mouse_bytes(b, cell, action, *modifiers, modes));
                        self.button = pressed.then_some(b);
                        self.last = Some(cell);
                    } else if b == 0 && *pressed {
                        self.selection = Some((cell, cell));
                        self.selecting = true;
                    } else if b == 0 {
                        self.selecting = false;
                        if self.selection.is_some_and(|(a, b)| a == b) {
                            self.selection = None;
                        }
                    }
                }
                egui::Event::PointerMoved(pos) => {
                    let cell = at(*pos);
                    if self.selecting {
                        self.selection = self.selection.map(|(a, _)| (a, cell));
                    } else if rect.contains(*pos) && self.last != Some(cell) {
                        let action = match self.button {
                            Some(_) => MouseAction::Drag,
                            None => MouseAction::Move,
                        };
                        let b = self.button.unwrap_or(0);
                        send.extend(mouse_bytes(b, cell, action, now, modes));
                        self.last = Some(cell);
                    }
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    ..
                } if response.contains_pointer() => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / ch,
                        egui::MouseWheelUnit::Page => delta.y * (sr + 1) as f32,
                    };
                    self.wheel += lines;
                    let n = self.wheel.trunc();
                    self.wheel -= n;
                    let n = n as i32;
                    if n == 0 {
                        continue;
                    }
                    let up = n > 0;
                    let cell = self.last.unwrap_or_default();
                    for _ in 0..n.unsigned_abs().min(MAX_ROWS as u32) {
                        if modes & mode::MOUSE != 0 {
                            let b = if up { 64 } else { 65 };
                            send.extend(mouse_bytes(
                                b,
                                cell,
                                MouseAction::Press,
                                *modifiers,
                                modes,
                            ));
                        } else if modes & mode::ALT_SCREEN != 0
                            && modes & mode::ALTERNATE_SCROLL != 0
                        {
                            let key = if up { Key::ArrowUp } else { Key::ArrowDown };
                            send.extend(key_bytes(key, Modifiers::NONE, modes, mac));
                        }
                    }
                    if modes & (mode::MOUSE | mode::ALT_SCREEN) == 0 {
                        self.out.push(scroll_message(n));
                    }
                }
                _ if !focused => {}
                egui::Event::Text(t) => {
                    // Alt with a character is Escape and the character, as in xterm. On
                    // macOS, Option makes other characters, so the text stays.
                    let esc = now.alt && !mac && t.chars().count() == 1;
                    send.push(format!("{}{t}", if esc { "\x1b" } else { "" }).into_bytes());
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let copy = !mac
                        && *key == Key::C
                        && modifiers.ctrl
                        && !modifiers.shift
                        && !modifiers.alt;
                    match self.selection.filter(|(a, b)| copy && a != b) {
                        Some((a, b)) => {
                            ui.ctx().copy_text(selected_text(&self.screen, a, b));
                            self.selection = None;
                        }
                        None => send.extend(key_bytes(*key, *modifiers, modes, mac)),
                    }
                }
                egui::Event::Paste(t) => send.push(paste_bytes(t, modes)),
                egui::Event::Copy => {
                    if let Some((a, b)) = self.selection {
                        ui.ctx().copy_text(selected_text(&self.screen, a, b));
                    }
                }
                _ => {}
            }
        }
        for bytes in send {
            self.type_bytes(&bytes);
        }
    }

    fn paint(&self, ui: &egui::Ui, rect: Rect, font: &FontId, (cw, ch): (f32, f32)) {
        let p = ui.painter_at(rect);
        let s = &self.screen;
        p.rect_filled(rect, 0.0, rgb(s.bg));
        let cell = |col: u16, row: u16, cols: u16| {
            let min = rect.min + egui::vec2(col as f32 * cw, row as f32 * ch);
            Rect::from_min_size(min, egui::vec2(cols as f32 * cw, ch))
        };
        for (row, spans) in s.lines.iter().enumerate() {
            for span in spans {
                let r = cell(span.col, row as u16, span.cols);
                if span.bg != s.bg {
                    p.rect_filled(r, 0.0, rgb(span.bg));
                }
                let fg = rgb(span.fg);
                if span.text.is_ascii() || one_cell(span) {
                    draw_text(&p, r.min, &span.text, font, fg, span.style);
                } else {
                    // One character for each cell, so that a glyph of another font keeps the grid.
                    for (i, c) in span.text.chars().enumerate() {
                        let at = r.min + egui::vec2(i as f32 * cw, 0.0);
                        draw_text(&p, at, &c.to_string(), font, fg, span.style);
                    }
                }
                if span.style & style::UNDERLINE != 0 {
                    p.hline(r.x_range(), r.bottom() - 1.5, Stroke::new(1.0, fg));
                }
                if span.style & style::STRIKEOUT != 0 {
                    p.hline(r.x_range(), r.center().y, Stroke::new(1.0, fg));
                }
            }
        }
        if let Some((a, b)) = self.selection {
            let (start, end) = (a.min(b), a.max(b));
            let fill = ui.visuals().selection.bg_fill.gamma_multiply(0.5);
            for row in start.row..=end.row {
                let from = if row == start.row { start.col } else { 0 };
                let to = if row == end.row { end.col + 1 } else { s.cols };
                p.rect_filled(cell(from, row, to.saturating_sub(from)), 0.0, fill);
            }
        }
        let c = cell(s.cursor_col, s.cursor_row, 1);
        let color = rgb(s.fg);
        if self.exit.is_none() {
            match (s.cursor_shape, self.focused) {
                (cursor::HIDDEN, _) => {}
                (cursor::BLOCK, true) => {
                    p.rect_filled(c, 0.0, color.gamma_multiply(0.6));
                }
                (cursor::UNDERLINE, _) => {
                    p.rect_filled(
                        Rect::from_min_max(egui::pos2(c.left(), c.bottom() - 2.0), c.max),
                        0.0,
                        color,
                    );
                }
                (cursor::BEAM, _) => {
                    p.rect_filled(
                        Rect::from_min_max(c.min, egui::pos2(c.left() + 2.0, c.bottom())),
                        0.0,
                        color,
                    );
                }
                _ => {
                    p.rect_stroke(c, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
                }
            }
        }
        if s.scrolled > 0 {
            // On a box of the text colour, so that the hint stays clear over the text.
            let text = format!("{} of {} lines up", s.scrolled, s.history);
            let galley = p.layout_no_wrap(text, FontId::proportional(11.0), rgb(s.bg));
            let at = rect.right_top() + egui::vec2(-6.0 - galley.size().x, 4.0);
            let back = Rect::from_min_size(at, galley.size()).expand(3.0);
            p.rect_filled(back, 3.0, rgb(s.fg));
            p.galley(at, galley, rgb(s.bg));
        }
        if self.focused {
            let stroke = ui.visuals().selection.stroke;
            p.rect_stroke(rect, 0.0, stroke, StrokeKind::Inside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The screen of `fixtures/terminal/screen-v1.hex`, as `crates/daemon/src/screen.rs` has it.
    fn sample() -> Screen {
        let span = |col, cols, fg, style, text: &str| Span {
            col,
            cols,
            fg,
            bg: 0x181818,
            style,
            text: text.into(),
        };
        Screen {
            cols: 6,
            rows: 2,
            cursor_col: 2,
            cursor_row: 1,
            cursor_shape: cursor::BLOCK,
            modes: mode::APP_CURSOR | mode::BRACKETED_PASTE,
            fg: 0xd8d8d8,
            bg: 0x181818,
            scrolled: 0,
            history: 3,
            lines: vec![
                vec![
                    span(0, 2, 0xd8d8d8, 0, "$ "),
                    span(2, 2, 0xac4242, style::BOLD, "ok"),
                ],
                vec![span(0, 2, 0xd8d8d8, style::WIDE, "界")],
            ],
        }
    }

    fn fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/terminal/screen-v1.hex"
        );
        let text = std::fs::read_to_string(path).unwrap();
        let hex: String = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .flat_map(|l| l.split_whitespace())
            .collect();
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    /// A screen message with the rows `rows` of `s`, as the daemon writes it.
    fn message(s: &Screen, rows: &[u16]) -> Vec<u8> {
        let mut m = vec![MSG_SCREEN];
        for v in [s.cols, s.rows, s.cursor_col, s.cursor_row] {
            m.extend(v.to_be_bytes());
        }
        m.push(s.cursor_shape);
        for v in [s.modes, s.fg, s.bg, s.scrolled, s.history] {
            m.extend(v.to_be_bytes());
        }
        m.extend((rows.len() as u16).to_be_bytes());
        for &row in rows {
            let spans = &s.lines[row as usize];
            m.extend(row.to_be_bytes());
            m.extend((spans.len() as u16).to_be_bytes());
            for sp in spans {
                for v in [sp.col, sp.cols] {
                    m.extend(v.to_be_bytes());
                }
                m.extend(sp.fg.to_be_bytes());
                m.extend(sp.bg.to_be_bytes());
                m.push(sp.style);
                m.extend((sp.text.len() as u16).to_be_bytes());
                m.extend(sp.text.as_bytes());
            }
        }
        m
    }

    #[test]
    fn the_shared_fixture_reads_as_the_sample_screen() {
        let mut s = Screen::default();
        assert_eq!(s.apply(&fixture()), Ok(Incoming::Screen));
        assert_eq!(s, sample());
        assert_eq!(message(&sample(), &[0, 1]), fixture());
    }

    #[test]
    fn a_later_message_changes_its_rows_and_a_bad_one_changes_nothing() {
        let mut s = Screen::default();
        s.apply(&fixture()).unwrap();
        let mut next = sample();
        next.lines[1] = vec![Span {
            col: 3,
            cols: 3,
            fg: 1,
            bg: 2,
            style: 0,
            text: "abc".into(),
        }];
        next.cursor_col = 5;
        s.apply(&message(&next, &[1])).unwrap();
        assert_eq!(s, next);
        let good = message(&next, &[0]);
        let mut wide = next.clone();
        wide.lines[0] = vec![Span {
            col: 5,
            cols: 2,
            text: "xy".into(),
            ..Span::default()
        }];
        let mut tall = next.clone();
        tall.rows = 1;
        let bad = [
            vec![],
            good[..good.len() - 1].to_vec(),
            [&good[..], &[0]].concat(),
            message(&wide, &[0]),
            message(&tall, &[1]),
            vec![9, 1, 2],
            vec![MSG_EXIT, 0, 0],
        ];
        for b in bad {
            assert!(s.apply(&b).is_err(), "{b:?}");
            assert_eq!(s, next);
        }
        let mut t = Term::new("t1", "shell");
        t.receive(&[&[MSG_TITLE][..], "vim ✓".as_bytes()].concat())
            .unwrap();
        t.receive(&[MSG_EXIT, 0xff, 0xff, 0xff, 0xf7]).unwrap();
        assert_eq!((t.title.as_str(), t.exit), ("vim ✓", Some(-9)));
        // A new size starts a new screen.
        let mut small = next.clone();
        (small.cols, small.rows, small.lines) = (4, 1, vec![vec![]]);
        s.apply(&message(&small, &[])).unwrap();
        assert_eq!(s, small);
    }

    #[test]
    fn the_client_messages_have_the_protocol_layout() {
        assert_eq!(input_message(b"ls\r"), b"\x01ls\r");
        assert_eq!(resize_message(300, 40), [2, 1, 44, 0, 40]);
        assert_eq!(scroll_message(-3), [3, 0xff, 0xff, 0xff, 0xfd]);
    }

    #[test]
    fn keys_give_the_xterm_bytes() {
        let none = Modifiers::NONE;
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..none
        };
        let mac_cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..none
        };
        let alt = Modifiers { alt: true, ..none };
        let shift = Modifiers {
            shift: true,
            ..none
        };
        let k =
            |key, m, modes| key_bytes(key, m, modes, false).map(|b| String::from_utf8(b).unwrap());
        let cases = [
            (Key::ArrowUp, none, 0, Some("\x1b[A")),
            (Key::ArrowUp, none, mode::APP_CURSOR, Some("\x1bOA")),
            (Key::ArrowLeft, ctrl, mode::APP_CURSOR, Some("\x1b[1;5D")),
            (Key::Home, shift, 0, Some("\x1b[1;2H")),
            (Key::Delete, none, 0, Some("\x1b[3~")),
            (Key::PageUp, alt, 0, Some("\x1b[5;3~")),
            (Key::F1, none, 0, Some("\x1bOP")),
            (Key::F5, none, 0, Some("\x1b[15~")),
            (Key::F12, ctrl, 0, Some("\x1b[24;5~")),
            (Key::Enter, none, 0, Some("\r")),
            (Key::Enter, none, mode::LINE_FEED_NEW_LINE, Some("\r\n")),
            (Key::Enter, alt, 0, Some("\x1b\r")),
            (Key::Tab, none, 0, Some("\t")),
            (Key::Tab, shift, 0, Some("\x1b[Z")),
            (Key::Backspace, none, 0, Some("\x7f")),
            (Key::Backspace, ctrl, 0, Some("\x08")),
            (Key::Escape, none, 0, Some("\x1b")),
            (Key::C, ctrl, 0, Some("\x03")),
            (Key::C, ctrl, 0, Some("\x03")),
            (Key::D, ctrl, 0, Some("\x04")),
            (Key::OpenBracket, ctrl, 0, Some("\x1b")),
            (Key::Space, ctrl, 0, Some("\0")),
            // Shortcuts of the browser and text keys give no bytes.
            (Key::V, ctrl, 0, None),
            (
                Key::C,
                Modifiers {
                    shift: true,
                    ..ctrl
                },
                0,
                None,
            ),
            (Key::C, mac_cmd, 0, None),
            (Key::ArrowUp, mac_cmd, 0, None),
            (Key::A, none, 0, None),
            (Key::A, alt, 0, None),
            (Key::Space, none, 0, None),
        ];
        for (key, m, modes, want) in cases {
            assert_eq!(k(key, m, modes).as_deref(), want, "{key:?} {m:?} {modes}");
        }
        // On macOS, Control+V is a key and Command+V pastes.
        let mac = |key, m| key_bytes(key, m, 0, true);
        assert_eq!(mac(Key::V, ctrl), Some(vec![0x16]));
        assert_eq!(mac(Key::V, mac_cmd), None);
    }

    #[test]
    fn paste_and_mouse_reports() {
        assert_eq!(paste_bytes("a\nb\r\nc", 0), b"a\rb\rc");
        let evil = "rm x\x1b[201~\nreboot\n";
        let pasted = String::from_utf8(paste_bytes(evil, mode::BRACKETED_PASTE)).unwrap();
        assert_eq!(pasted, "\x1b[200~rm x[201~\rreboot\r\x1b[201~");
        let nested = "echo hi\x1b[20\x1b[201~1~\rid\r";
        let pasted = paste_bytes(nested, mode::BRACKETED_PASTE);
        assert_eq!(pasted, b"\x1b[200~echo hi[20[201~1~\rid\r\x1b[201~");
        assert_eq!(pasted.windows(6).filter(|w| w == b"\x1b[201~").count(), 1);
        let none = Modifiers::NONE;
        let ctrl = Modifiers { ctrl: true, ..none };
        let at = Cell { row: 4, col: 9 };
        let m = |b, action, mods, modes| mouse_bytes(b, at, action, mods, modes);
        let sgr = mode::MOUSE_CLICK | mode::SGR_MOUSE;
        assert_eq!(m(0, MouseAction::Press, none, 0), None);
        assert_eq!(
            m(0, MouseAction::Press, none, sgr).unwrap(),
            b"\x1b[<0;10;5M"
        );
        assert_eq!(
            m(2, MouseAction::Release, ctrl, sgr).unwrap(),
            b"\x1b[<18;10;5m"
        );
        assert_eq!(
            m(64, MouseAction::Press, none, sgr).unwrap(),
            b"\x1b[<64;10;5M"
        );
        // Drag needs mode 1002 or 1003, and a move needs 1003.
        assert_eq!(m(0, MouseAction::Drag, none, sgr), None);
        let drag = sgr | mode::MOUSE_DRAG;
        assert_eq!(
            m(0, MouseAction::Drag, none, drag).unwrap(),
            b"\x1b[<32;10;5M"
        );
        assert_eq!(m(0, MouseAction::Move, none, drag), None);
        let motion = sgr | mode::MOUSE_MOTION;
        assert_eq!(
            m(0, MouseAction::Move, none, motion).unwrap(),
            b"\x1b[<35;10;5M"
        );
        // The old form: 32 + the code, 32 + the column, 32 + the row. A release is code 3.
        let click = mode::MOUSE_CLICK;
        assert_eq!(
            m(0, MouseAction::Press, none, click).unwrap(),
            [27, b'[', b'M', 32, 42, 37]
        );
        assert_eq!(
            m(0, MouseAction::Release, none, click).unwrap(),
            [27, b'[', b'M', 35, 42, 37]
        );
        let far = Cell { row: 0, col: 300 };
        assert_eq!(mouse_bytes(0, far, MouseAction::Press, none, click), None);
        let utf8 = click | mode::UTF8_MOUSE;
        let got = mouse_bytes(0, far, MouseAction::Press, none, utf8).unwrap();
        assert_eq!(got, [&b"\x1b[M "[..], "ō".as_bytes(), b"!"].concat());
    }

    #[test]
    fn a_selection_takes_the_text_of_its_cells() {
        let s = sample();
        let cell = |row, col| Cell { row, col };
        assert_eq!(selected_text(&s, cell(0, 2), cell(0, 3)), "ok");
        assert_eq!(selected_text(&s, cell(1, 1), cell(0, 0)), "$ ok\n界");
        assert_eq!(selected_text(&s, cell(0, 4), cell(0, 5)), "");
        assert_eq!(selected_text(&s, cell(1, 0), cell(9, 9)), "界");
    }

    #[test]
    fn a_cell_with_marks_is_one_cluster_in_its_cell() {
        let mut s = sample();
        s.cols = 6;
        let span = |col, cols, text: &str| Span {
            col,
            cols,
            bg: 0x181818,
            text: text.into(),
            ..Span::default()
        };
        s.lines[0] = vec![span(0, 1, "e\u{301}"), span(1, 2, "ab"), span(3, 1, "x")];
        let cells = row_cells(&s, 0);
        assert_eq!(cells[..4], ["e\u{301}", "a", "b", "x"]);
        let cell = |row, col| Cell { row, col };
        assert_eq!(selected_text(&s, cell(0, 1), cell(0, 3)), "abx");
        assert_eq!(selected_text(&s, cell(0, 0), cell(0, 0)), "e\u{301}");
    }

    struct Frame {
        ctx: egui::Context,
        pos: Pos2,
    }

    impl Frame {
        fn new(os: egui::os::OperatingSystem) -> Frame {
            let ctx = egui::Context::default();
            ctx.set_os(os);
            Frame {
                ctx,
                pos: Pos2::ZERO,
            }
        }

        fn run(&mut self, t: &mut Term, events: Vec<egui::Event>) -> Vec<String> {
            let raw = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 200.0))),
                ..Default::default()
            };
            let mut out = self.ctx.run_ui(raw, |ui| {
                t.ui(ui);
            });
            out.textures_delta.clear();
            out.platform_output
                .commands
                .into_iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(s) => Some(s),
                    _ => None,
                })
                .collect()
        }

        fn click(&self, pressed: bool) -> egui::Event {
            egui::Event::PointerButton {
                pos: self.pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }
        }
    }

    fn open_term() -> Term {
        let mut t = Term::new("t1", "shell");
        t.opened();
        t.receive(&fixture()).unwrap();
        t.out.clear();
        t
    }

    fn select_and_press(
        os: egui::os::OperatingSystem,
        key: Key,
        m: Modifiers,
    ) -> (Vec<Vec<u8>>, Vec<String>, bool) {
        let mut f = Frame::new(os);
        let mut t = open_term();
        f.pos = egui::pos2(2.0, 2.0);
        f.run(&mut t, vec![f.click(true)]);
        f.pos = egui::pos2(40.0, 2.0);
        f.run(
            &mut t,
            vec![egui::Event::PointerMoved(f.pos), f.click(false)],
        );
        t.out.clear();
        let had = t.selection.is_some();
        assert!(had, "the drag made no selection");
        let press = egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: m,
        };
        let copied = f.run(&mut t, vec![press]);
        let sent: Vec<_> = t
            .out
            .iter()
            .filter(|m| m[0] == MSG_INPUT)
            .cloned()
            .collect();
        (sent, copied, t.selection.is_some())
    }

    #[test]
    fn control_c_copies_a_selection_on_other_systems_and_never_cuts() {
        use egui::os::OperatingSystem::{Mac, Windows};
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let (sent, copied, kept) = select_and_press(Windows, Key::C, ctrl);
        assert_eq!(
            (sent.len(), copied.len(), kept),
            (0, 1, false),
            "{copied:?}"
        );
        assert!(!copied[0].is_empty());
        let (sent, copied, _) = select_and_press(Windows, Key::X, ctrl);
        assert_eq!((sent, copied.len()), (vec![vec![1, 0x18]], 0));
        let (sent, copied, _) = select_and_press(Mac, Key::C, ctrl);
        assert_eq!((sent, copied.len()), (vec![vec![1, 3]], 0));
    }

    #[test]
    fn control_c_with_no_selection_sends_the_interrupt() {
        let mut f = Frame::new(egui::os::OperatingSystem::Windows);
        let mut t = open_term();
        f.pos = egui::pos2(2.0, 2.0);
        f.run(&mut t, vec![f.click(true), f.click(false)]);
        let ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let key = egui::Event::Key {
            key: Key::C,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: ctrl,
        };
        let copied = f.run(&mut t, vec![key, egui::Event::Copy]);
        assert!(copied.is_empty());
        assert!(t.out.contains(&vec![1, 3]), "{:?}", t.out);
    }

    #[test]
    fn a_focus_report_keeps_the_selection() {
        let mut f = Frame::new(egui::os::OperatingSystem::Windows);
        let mut t = open_term();
        t.screen.modes |= mode::FOCUS_IN_OUT;
        f.pos = egui::pos2(2.0, 2.0);
        f.run(&mut t, vec![f.click(true)]);
        f.pos = egui::pos2(40.0, 2.0);
        f.run(
            &mut t,
            vec![egui::Event::PointerMoved(f.pos), f.click(false)],
        );
        assert!(t.out.contains(&vec![1, 0x1b, b'[', b'I']), "{:?}", t.out);
        assert!(t.selection.is_some());
    }
}
