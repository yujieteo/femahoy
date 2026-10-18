//! A small JSON reader (RFC 8259) for the event stream of the agent (`chat.rs`).

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

const MAX_DEPTH: usize = 64;

impl Json {
    pub(crate) fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser(text.as_bytes(), 0);
        let v = p.value(0)?;
        p.space();
        match p.1 == p.0.len() {
            true => Ok(v),
            false => p.fail("text after the value"),
        }
    }

    pub(crate) fn at(&self, path: &[&str]) -> Option<&Json> {
        path.iter().try_fold(self, |v, key| match v {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        })
    }

    pub(crate) fn text(&self, path: &[&str]) -> Option<&str> {
        match self.at(path)? {
            Json::Text(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn flag(&self, path: &[&str]) -> Option<bool> {
        match self.at(path)? {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn list(&self, path: &[&str]) -> &[Json] {
        match self.at(path) {
            Some(Json::List(items)) => items,
            _ => &[],
        }
    }
}

struct Parser<'a>(&'a [u8], usize);

impl Parser<'_> {
    fn space(&mut self) {
        while self.0.get(self.1).is_some_and(|c| b" \t\r\n".contains(c)) {
            self.1 += 1;
        }
    }

    fn fail<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("{what} at byte {}", self.1))
    }

    fn eat(&mut self, word: &str) -> bool {
        self.space();
        let found = self.0[self.1..].starts_with(word.as_bytes());
        self.1 += if found { word.len() } else { 0 };
        found
    }

    fn items<T>(
        &mut self,
        end: &str,
        mut item: impl FnMut(&mut Self) -> Result<T, String>,
    ) -> Result<Vec<T>, String> {
        let mut items = Vec::new();
        if self.eat(end) {
            return Ok(items);
        }
        loop {
            items.push(item(self)?);
            if self.eat(end) {
                return Ok(items);
            }
            if !self.eat(",") {
                return self.fail(&format!("no ',' or '{end}'"));
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        self.space();
        let start = self.1;
        match self.0.get(self.1) {
            Some(b'{' | b'[') if depth == MAX_DEPTH => self.fail("too deep"),
            Some(b'{') => {
                self.1 += 1;
                Ok(Json::Object(self.items("}", |p| {
                    p.space();
                    let key = p.string()?;
                    match p.eat(":") {
                        true => Ok((key, p.value(depth + 1)?)),
                        false => p.fail("no ':'"),
                    }
                })?))
            }
            Some(b'[') => {
                self.1 += 1;
                Ok(Json::List(self.items("]", |p| p.value(depth + 1))?))
            }
            Some(b'"') => Ok(Json::Text(self.string()?)),
            _ if self.eat("true") => Ok(Json::Bool(true)),
            _ if self.eat("false") => Ok(Json::Bool(false)),
            _ if self.eat("null") => Ok(Json::Null),
            Some(b'-' | b'0'..=b'9') => {
                let more = |c: &u8| c.is_ascii_digit() || b"+-.eE".contains(c);
                while self.0.get(self.1).is_some_and(more) {
                    self.1 += 1;
                }
                let text = String::from_utf8_lossy(&self.0[start..self.1]).into_owned();
                match text.parse::<f64>() {
                    Ok(_) => Ok(Json::Number(text)),
                    Err(_) => self.fail("a bad number"),
                }
            }
            _ => self.fail("no JSON value"),
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.0.get(self.1..self.1 + 4).unwrap_or(b"x");
        let n = std::str::from_utf8(digits)
            .ok()
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()));
        let n = n.and_then(|d| u32::from_str_radix(d, 16).ok());
        self.1 += 4;
        n.map_or_else(|| self.fail("a bad \\u escape"), Ok)
    }

    fn string(&mut self) -> Result<String, String> {
        if self.0.get(self.1) != Some(&b'"') {
            return self.fail("no string");
        }
        self.1 += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.0.get(self.1).ok_or("an open string")?;
            self.1 += 1;
            let ch = match c {
                b'"' => break,
                0..=0x1F => return self.fail("a control character in a string"),
                b'\\' => {
                    let e = *self.0.get(self.1).ok_or("an open escape")?;
                    self.1 += 1;
                    match e {
                        b'"' | b'\\' | b'/' => e as char,
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let pair = (0xD800..0xDC00).contains(&hi)
                                && self.0[self.1..].starts_with(b"\\u");
                            let code = match pair {
                                true => {
                                    let at = self.1;
                                    self.1 += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                    } else {
                                        self.1 = at;
                                        0xFFFD
                                    }
                                }
                                false => hi,
                            };
                            char::from_u32(code).unwrap_or('\u{FFFD}')
                        }
                        _ => return self.fail("a bad escape"),
                    }
                }
                _ => {
                    out.push(c);
                    continue;
                }
            };
            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
        }
        String::from_utf8(out).map_err(|_| "a string that is not UTF-8".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrogates_and_hex_digits_in_escapes() {
        let text = |s: &str| Json::parse(s).unwrap();
        assert_eq!(text(r#""\ud83d\u0041""#), Json::Text("\u{FFFD}A".into()));
        assert_eq!(text(r#""\ud83d\ude00""#), Json::Text("😀".into()));
        assert_eq!(text(r#""\ud83dx""#), Json::Text("\u{FFFD}x".into()));
        assert!(Json::parse(r#""\u+041""#).is_err());
        assert!(Json::parse(r#""\u-041""#).is_err());
    }

    #[test]
    fn reads_an_event_and_refuses_bad_json() {
        let text = r#"{"result":{"pane":{"focused":false,"pane_id":"w1:p2","scroll":{"rows":40},
            "label":"caf\u00e9 \ud83d\ude00 \"q\""}},"n":[1,-2.5e1,null,true]}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.text(&["result", "pane", "pane_id"]), Some("w1:p2"));
        assert_eq!(v.text(&["result", "pane", "label"]), Some("café 😀 \"q\""));
        assert_eq!(v.flag(&["result", "pane", "focused"]), Some(false));
        let n = |s: &str| Json::Number(s.into());
        let want = [n("1"), n("-2.5e1"), Json::Null, Json::Bool(true)];
        assert_eq!(v.list(&["n"]), want);
        assert_eq!(v.at(&["result", "missing"]), None);
        assert!(v.list(&["result"]).is_empty());
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        let bad = [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "\"open",
            "\"a\nb\"",
            "tru",
            "1 2",
            "\"\\x\"",
        ];
        for bad in bad.iter().chain([&deep.as_str()]) {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
    }
}
