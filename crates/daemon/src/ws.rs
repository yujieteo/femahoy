//! A small WebSocket layer (RFC 6455) for the terminal route: the key of the handshake
//! answer, and the frames in both directions. The daemon uses it as the server, and
//! `client.rs` uses it as the client for the CLI and the tests.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::pty::Terminal;
use crate::screen::{self, ClientMessage};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MAX_MESSAGE: usize = 1 << 20;

pub(crate) const OP_CONTINUATION: u8 = 0;
pub(crate) const OP_TEXT: u8 = 1;
pub(crate) const OP_BINARY: u8 = 2;
pub(crate) const OP_CLOSE: u8 = 8;
pub(crate) const OP_PING: u8 = 9;
pub(crate) const OP_PONG: u8 = 10;

#[derive(Debug, PartialEq, Eq)]
pub enum Incoming {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close(Option<u16>),
}

#[derive(Debug)]
pub enum WsError {
    Io(io::Error),
    Protocol(u16, &'static str),
}

impl From<io::Error> for WsError {
    fn from(e: io::Error) -> Self {
        WsError::Io(e)
    }
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WsError::Io(e) => write!(f, "{e}"),
            WsError::Protocol(code, why) => write!(f, "{why} (close code {code})"),
        }
    }
}

pub(crate) fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{key}{GUID}").as_bytes()))
}

pub(crate) fn valid_key(key: &str) -> bool {
    key.len() == 24
        && key.is_ascii()
        && key.ends_with("==")
        && key[..22]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub(crate) fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(crate) fn write_frame(
    w: &mut impl Write,
    opcode: u8,
    payload: &[u8],
    mask: Option<[u8; 4]>,
) -> io::Result<()> {
    let mut head = vec![0x80 | opcode];
    let bit = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n @ 0..=125 => head.push(bit | n as u8),
        n @ 126..=0xFFFF => {
            head.push(bit | 126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(bit | 127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            head.extend_from_slice(&key);
            head.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
            w.write_all(&head)?;
        }
        None => {
            w.write_all(&head)?;
            w.write_all(payload)?;
        }
    }
    w.flush()
}

pub(crate) fn close_payload(code: u16) -> Vec<u8> {
    code.to_be_bytes().to_vec()
}

struct Frame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

fn read_frame(r: &mut impl Read, masked: bool, room: usize) -> Result<Frame, WsError> {
    let mut head = [0u8; 2];
    r.read_exact(&mut head)?;
    if head[0] & 0x70 != 0 {
        return Err(WsError::Protocol(1002, "a reserved bit is set"));
    }
    let (fin, opcode) = (head[0] & 0x80 != 0, head[0] & 0x0F);
    if (head[1] & 0x80 != 0) != masked {
        let why = if masked {
            "a client frame is not masked"
        } else {
            "a server frame is masked"
        };
        return Err(WsError::Protocol(1002, why));
    }
    let len = match head[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => u64::from(n),
    };
    let control = opcode >= OP_CLOSE;
    if control && (len > 125 || !fin) {
        return Err(WsError::Protocol(1002, "a bad control frame"));
    }
    if len > room as u64 {
        return Err(WsError::Protocol(1009, "the message is too large"));
    }
    let mut key = [0u8; 4];
    if masked {
        r.read_exact(&mut key)?;
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= key[i % 4];
        }
    }
    Ok(Frame {
        fin,
        opcode,
        payload,
    })
}

/// Read one message. Fragments join; `masked` is true on the server side; `data` keeps fragments.
pub(crate) fn read_message(
    r: &mut impl Read,
    masked: bool,
    data: &mut Option<Vec<u8>>,
) -> Result<Incoming, WsError> {
    loop {
        let room = MAX_MESSAGE - data.as_ref().map_or(0, Vec::len);
        let frame = read_frame(r, masked, room)?;
        match frame.opcode {
            OP_PING => return Ok(Incoming::Ping(frame.payload)),
            OP_PONG => return Ok(Incoming::Pong),
            OP_CLOSE => {
                let code = (frame.payload.len() >= 2)
                    .then(|| u16::from_be_bytes([frame.payload[0], frame.payload[1]]));
                return Ok(Incoming::Close(code));
            }
            OP_TEXT => return Err(WsError::Protocol(1003, "text messages are not used")),
            OP_BINARY if data.is_none() => *data = Some(frame.payload),
            OP_CONTINUATION if data.is_some() => {
                data.as_mut().expect("checked").extend(frame.payload);
            }
            _ => return Err(WsError::Protocol(1002, "a bad frame order or opcode")),
        }
        if frame.fin {
            return Ok(Incoming::Binary(data.take().unwrap_or_default()));
        }
    }
}

const PING_EVERY: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Serve the terminal `t` on an upgraded connection until the client or the terminal ends.
pub(crate) fn attach(stream: TcpStream, t: Arc<Terminal>) {
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let out = Arc::new(Mutex::new(stream));
    let gone = Arc::new(AtomicBool::new(false));
    let send = |op: u8, payload: &[u8]| {
        let mut stream = out.lock().unwrap();
        if gone.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if op == OP_CLOSE {
            gone.store(true, Ordering::SeqCst);
        }
        write_frame(&mut *stream, op, payload, None)
    };
    std::thread::scope(|s| {
        s.spawn(|| {
            let mut r = io::BufReader::new(read_half);
            let mut data = None;
            let close = loop {
                let m = match read_message(&mut r, true, &mut data) {
                    Ok(Incoming::Binary(m)) => m,
                    Ok(Incoming::Ping(p)) => match send(OP_PONG, &p) {
                        Ok(()) => continue,
                        Err(_) => break None,
                    },
                    Ok(Incoming::Pong) => continue,
                    Ok(Incoming::Close(_)) => break Some(1000),
                    Err(WsError::Protocol(code, _)) => break Some(code),
                    Err(WsError::Io(_)) => break None,
                };
                // Input to an ended terminal changes nothing. The writer sends the exit.
                let _ = match screen::parse_client(&m) {
                    Ok(ClientMessage::Input(bytes)) => t.write(&bytes),
                    Ok(ClientMessage::Resize(cols, rows)) => t.resize(cols, rows),
                    Ok(ClientMessage::Scroll(lines)) => {
                        t.scroll(lines);
                        Ok(())
                    }
                    Err(_) => break Some(1003),
                };
            };
            if let Some(code) = close {
                let _ = send(OP_CLOSE, &close_payload(code));
            }
            gone.store(true, Ordering::SeqCst);
        });
        let mut encoder = screen::Encoder::default();
        let (mut seen, mut title) = (u64::MAX, String::new());
        let mut ping = Instant::now() + PING_EVERY;
        while !gone.load(Ordering::SeqCst) {
            let (version, ended) = t.wait(seen, Duration::from_millis(250));
            let mut sent = Ok(());
            if version != seen {
                seen = version;
                let info = t.info();
                if let Some(m) = encoder.encode(&t.screen()) {
                    sent = sent.and_then(|()| send(OP_BINARY, &m));
                }
                if info.title != title {
                    title = info.title;
                    sent = sent.and_then(|()| send(OP_BINARY, &screen::title_message(&title)));
                }
                if ended {
                    let code = info.exit.unwrap_or(-1);
                    let _ = sent.and_then(|()| send(OP_BINARY, &screen::exit_message(code)));
                    let _ = send(OP_CLOSE, &close_payload(1000));
                    break;
                }
            }
            if Instant::now() >= ping {
                ping = Instant::now() + PING_EVERY;
                sent = sent.and_then(|()| send(OP_PING, b""));
            }
            if sent.is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        let _ = out.lock().unwrap().shutdown(std::net::Shutdown::Both);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accept_key_of_rfc_6455() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
        assert!(valid_key("dGhlIHNhbXBsZSBub25jZQ=="));
        assert!(!valid_key("short=="));
        assert!(!valid_key("dGhlIHNhbXBsZSBub25jZQ!!"));
    }

    #[test]
    fn sha1_and_base64_of_known_texts() {
        let hex = |b: [u8; 20]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        let long = vec![b'a'; 1000];
        assert_eq!(hex(sha1(&long)), "291e9a6c66994949b57ba5e650361e98fc36b1ba");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn frames_go_both_ways_with_all_length_forms() {
        for n in [0, 5, 125, 126, 300, 70_000] {
            let payload: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            let mut wire = Vec::new();
            write_frame(&mut wire, OP_BINARY, &payload, Some([1, 2, 3, 4])).unwrap();
            let got = read_message(&mut wire.as_slice(), true, &mut None).unwrap();
            assert_eq!(got, Incoming::Binary(payload.clone()));
            let mut wire = Vec::new();
            write_frame(&mut wire, OP_BINARY, &payload, None).unwrap();
            assert_eq!(
                read_message(&mut wire.as_slice(), false, &mut None).unwrap(),
                Incoming::Binary(payload)
            );
        }
    }

    #[test]
    fn fragments_join_and_control_frames_come_through() {
        let key = Some([9, 8, 7, 6]);
        let mut wire = Vec::new();
        let mut first = Vec::new();
        write_frame(&mut first, OP_BINARY, b"ab", key).unwrap();
        first[0] &= 0x7F;
        wire.extend(first);
        // A ping between the fragments comes first, and the message stays whole.
        write_frame(&mut wire, OP_PING, b"x", key).unwrap();
        write_frame(&mut wire, OP_CONTINUATION, b"cd", key).unwrap();
        write_frame(&mut wire, OP_CLOSE, &close_payload(1000), key).unwrap();
        let (mut r, mut data) = (wire.as_slice(), None);
        let mut next = || read_message(&mut r, true, &mut data).unwrap();
        assert_eq!(next(), Incoming::Ping(b"x".to_vec()));
        assert_eq!(next(), Incoming::Binary(b"abcd".to_vec()));
        assert_eq!(next(), Incoming::Close(Some(1000)));
    }

    #[test]
    fn bad_frames_close_with_their_codes() {
        let code = |wire: Vec<u8>| match read_message(&mut wire.as_slice(), true, &mut None) {
            Err(WsError::Protocol(code, _)) => code,
            other => panic!("{other:?}"),
        };
        let mut unmasked = Vec::new();
        write_frame(&mut unmasked, OP_BINARY, b"a", None).unwrap();
        assert_eq!(code(unmasked), 1002);
        let mut text = Vec::new();
        write_frame(&mut text, OP_TEXT, b"a", Some([0; 4])).unwrap();
        assert_eq!(code(text), 1003);
        let mut stray = Vec::new();
        write_frame(&mut stray, OP_CONTINUATION, b"a", Some([0; 4])).unwrap();
        assert_eq!(code(stray), 1002);
        let mut huge = vec![0x82, 0x80 | 127];
        huge.extend_from_slice(&((MAX_MESSAGE as u64) + 1).to_be_bytes());
        assert_eq!(code(huge), 1009);
    }
}
