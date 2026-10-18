//! The terminal protocol `fh.term.v1`: the binary messages on the WebSocket of a terminal.
//! `docs/terminal.md` has the message layout. The daemon keeps the screen and sends the
//! rows that changed. The client sends its keys as bytes, its size and its scroll requests.

pub const PROTOCOL: &str = "fh.term.v1";
/// A browser cannot set the `Authorization` header of a WebSocket. It sends the token as a
/// second subprotocol item with this prefix. The daemon never sends that item back.
pub const TOKEN_PROTOCOL: &str = "fh.token.";

pub const MSG_SCREEN: u8 = 1;
pub const MSG_TITLE: u8 = 2;
pub const MSG_EXIT: u8 = 3;
pub const MSG_INPUT: u8 = 1;
pub const MSG_RESIZE: u8 = 2;
pub const MSG_SCROLL: u8 = 3;

pub mod mode {
    pub const APP_CURSOR: u32 = 1;
    pub const APP_KEYPAD: u32 = 1 << 1;
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
}

pub mod style {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 1 << 1;
    pub const UNDERLINE: u8 = 1 << 2;
    pub const STRIKEOUT: u8 = 1 << 3;
    pub const WIDE: u8 = 1 << 4;
}

pub mod cursor {
    pub const HIDDEN: u8 = 0;
    pub const BLOCK: u8 = 1;
    pub const UNDERLINE: u8 = 2;
    pub const BEAM: u8 = 3;
    pub const HOLLOW: u8 = 4;
}

pub const MAX_COLS: u16 = 500;
pub const MAX_ROWS: u16 = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub col: u16,
    pub cols: u16,
    pub fg: u32,
    pub bg: u32,
    pub style: u8,
    pub text: String,
}

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

#[derive(Debug, PartialEq, Eq)]
pub enum ClientMessage {
    Input(Vec<u8>),
    Resize(u16, u16),
    Scroll(i32),
}

pub fn parse_client(bytes: &[u8]) -> Result<ClientMessage, String> {
    let (kind, rest) = bytes.split_first().ok_or("an empty message")?;
    match (*kind, rest.len()) {
        (MSG_INPUT, _) => Ok(ClientMessage::Input(rest.to_vec())),
        (MSG_RESIZE, 4) => Ok(ClientMessage::Resize(
            u16::from_be_bytes([rest[0], rest[1]]),
            u16::from_be_bytes([rest[2], rest[3]]),
        )),
        (MSG_SCROLL, 4) => Ok(ClientMessage::Scroll(i32::from_be_bytes([
            rest[0], rest[1], rest[2], rest[3],
        ]))),
        (k, n) => Err(format!("an unknown message type {k} with {n} bytes")),
    }
}

pub fn input_message(bytes: &[u8]) -> Vec<u8> {
    let mut m = vec![MSG_INPUT];
    m.extend_from_slice(bytes);
    m
}

pub fn resize_message(cols: u16, rows: u16) -> Vec<u8> {
    let mut m = vec![MSG_RESIZE];
    m.extend_from_slice(&cols.to_be_bytes());
    m.extend_from_slice(&rows.to_be_bytes());
    m
}

pub fn exit_message(code: i32) -> Vec<u8> {
    let mut m = vec![MSG_EXIT];
    m.extend_from_slice(&code.to_be_bytes());
    m
}

pub fn title_message(title: &str) -> Vec<u8> {
    let mut m = vec![MSG_TITLE];
    m.extend_from_slice(title.as_bytes());
    m
}

fn encode_row(spans: &[Span]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&(spans.len() as u16).to_be_bytes());
    for s in spans {
        b.extend_from_slice(&s.col.to_be_bytes());
        b.extend_from_slice(&s.cols.to_be_bytes());
        b.extend_from_slice(&s.fg.to_be_bytes());
        b.extend_from_slice(&s.bg.to_be_bytes());
        b.push(s.style);
        let mut end = s.text.len().min(u16::MAX as usize);
        while !s.text.is_char_boundary(end) {
            end -= 1;
        }
        b.extend_from_slice(&(end as u16).to_be_bytes());
        b.extend_from_slice(&s.text.as_bytes()[..end]);
    }
    b
}

/// The screen messages for one client. The first message names all rows. Each later one
/// names only the rows that changed.
#[derive(Default)]
pub struct Encoder {
    head: Vec<u8>,
    rows: Vec<Vec<u8>>,
}

impl Encoder {
    pub fn encode(&mut self, screen: &Screen) -> Option<Vec<u8>> {
        let mut head = Vec::new();
        for v in [
            screen.cols,
            screen.rows,
            screen.cursor_col,
            screen.cursor_row,
        ] {
            head.extend_from_slice(&v.to_be_bytes());
        }
        head.push(screen.cursor_shape);
        for v in [
            screen.modes,
            screen.fg,
            screen.bg,
            screen.scrolled,
            screen.history,
        ] {
            head.extend_from_slice(&v.to_be_bytes());
        }
        let rows: Vec<Vec<u8>> = screen.lines.iter().map(|l| encode_row(l)).collect();
        // The first 4 bytes of the head are the size.
        let all = self.head.get(..4) != head.get(..4) || self.rows.len() != rows.len();
        let changed: Vec<usize> = (0..rows.len())
            .filter(|&i| all || self.rows[i] != rows[i])
            .collect();
        if changed.is_empty() && head == self.head {
            return None;
        }
        let mut m = vec![MSG_SCREEN];
        m.extend_from_slice(&head);
        m.extend_from_slice(&(changed.len() as u16).to_be_bytes());
        for i in changed {
            m.extend_from_slice(&(i as u16).to_be_bytes());
            m.extend_from_slice(&rows[i]);
        }
        self.head = head;
        self.rows = rows;
        Some(m)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> Screen {
        let span = |col, cols, fg, bg, style, text: &str| Span {
            col,
            cols,
            fg,
            bg,
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
                    span(0, 2, 0xd8d8d8, 0x181818, 0, "$ "),
                    span(2, 2, 0xac4242, 0x181818, style::BOLD, "ok"),
                ],
                vec![span(0, 2, 0xd8d8d8, 0x181818, style::WIDE, "界")],
            ],
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_first_message_matches_the_shared_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/terminal/screen-v1.hex"
        );
        let want: String = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<String>()
            .split_whitespace()
            .collect();
        let got = Encoder::default().encode(&sample()).unwrap();
        assert_eq!(hex(&got), want);
    }

    #[test]
    fn later_messages_name_only_the_changed_rows() {
        let mut e = Encoder::default();
        let mut s = sample();
        e.encode(&s).unwrap();
        assert_eq!(e.encode(&s), None);
        s.lines[1][0].text = "x".into();
        s.lines[1][0].style = 0;
        s.lines[1][0].cols = 1;
        let m = e.encode(&s).unwrap();
        // The type, the head of 29 bytes, the row count 1 and the row index 1.
        assert_eq!(&m[30..34], &[0, 1, 0, 1]);
        // A move of the cursor alone sends the head with no row.
        s.cursor_col = 3;
        let m = e.encode(&s).unwrap();
        assert_eq!(&m[30..32], &[0, 0]);
        // A new size names all rows again.
        s.cols = 7;
        let m = e.encode(&s).unwrap();
        assert_eq!(&m[30..32], &[0, 2]);
    }

    #[test]
    fn a_span_text_never_exceeds_its_length_field() {
        let mut s = sample();
        s.lines[1][0].text = format!("a{}", "\u{301}".repeat(40_000));
        let m = Encoder::default().encode(&s).unwrap();
        let row1 = &m[m.len() - encode_row(&s.lines[1]).len()..];
        let len = u16::from_be_bytes([row1[2 + 13], row1[2 + 14]]) as usize;
        assert_eq!(row1.len(), 2 + 15 + len);
        assert!(std::str::from_utf8(&row1[17..]).is_ok());
    }

    #[test]
    fn client_messages_parse_and_refuse_bad_ones() {
        assert_eq!(
            parse_client(&input_message(b"ls\r")),
            Ok(ClientMessage::Input(b"ls\r".to_vec()))
        );
        assert_eq!(
            parse_client(&resize_message(120, 40)),
            Ok(ClientMessage::Resize(120, 40))
        );
        assert_eq!(
            parse_client(&[MSG_SCROLL, 0xff, 0xff, 0xff, 0xfd]),
            Ok(ClientMessage::Scroll(-3))
        );
        assert!(parse_client(&[]).is_err());
        assert!(parse_client(&[MSG_RESIZE, 1]).is_err());
        assert!(parse_client(&[9]).is_err());
    }
}
