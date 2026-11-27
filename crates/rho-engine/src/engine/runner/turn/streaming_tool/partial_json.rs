//! Reads the arguments of a tool call while they stream: the JSON seen so far, closed where it
//! breaks off. A value cut off mid-string keeps the text so far; a cut-off key, number, or literal
//! is left out.

use serde_json::{Map, Value};

/// How deep arrays and objects may nest, as `serde_json` allows; anything deeper is left out.
const MAX_DEPTH: usize = 128;

/// The value `json` holds so far, or `None` when not even its first value has started.
pub fn parse_prefix(json: &str) -> Option<Value> {
    let mut parser = Parser {
        bytes: json.as_bytes(),
        pos: 0,
        depth: 0,
    };
    parser.value().map(|parsed| parsed.value)
}

struct Parsed {
    value: Value,
    complete: bool,
}

impl Parsed {
    fn complete(value: Value) -> Self {
        Self { value, complete: true }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn skip_whitespace(&mut self) {
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_whitespace) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_whitespace();
        self.bytes.get(self.pos).copied()
    }

    fn value(&mut self) -> Option<Parsed> {
        match self.peek()? {
            b'{' | b'[' if self.depth == MAX_DEPTH => None,
            b'{' => Some(self.nested(Self::object)),
            b'[' => Some(self.nested(Self::array)),
            b'"' => {
                let (text, complete) = self.string()?;
                Some(Parsed {
                    value: Value::String(text),
                    complete,
                })
            }
            _ => self.scalar(),
        }
    }

    fn nested(&mut self, read: fn(&mut Self) -> Parsed) -> Parsed {
        self.depth += 1;
        let parsed = read(self);
        self.depth -= 1;
        parsed
    }

    fn object(&mut self) -> Parsed {
        self.pos += 1;
        let mut map = Map::new();
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Parsed::complete(Value::Object(map));
                }
                Some(b',') => self.pos += 1,
                _ => {}
            }
            if self.peek() != Some(b'"') {
                return Self::partial(Value::Object(map));
            }
            let Some((key, true)) = self.string() else {
                return Self::partial(Value::Object(map));
            };
            if self.peek() != Some(b':') {
                return Self::partial(Value::Object(map));
            }
            self.pos += 1;
            let Some(entry) = self.value() else {
                return Self::partial(Value::Object(map));
            };
            map.insert(key, entry.value);
            if !entry.complete {
                return Self::partial(Value::Object(map));
            }
        }
    }

    fn array(&mut self) -> Parsed {
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return Parsed::complete(Value::Array(items));
                }
                Some(b',') => self.pos += 1,
                None => return Self::partial(Value::Array(items)),
                Some(_) => {}
            }
            let Some(item) = self.value() else {
                return Self::partial(Value::Array(items));
            };
            items.push(item.value);
            if !item.complete {
                return Self::partial(Value::Array(items));
            }
        }
    }

    fn partial(value: Value) -> Parsed {
        Parsed { value, complete: false }
    }

    /// Reads the string at the cursor and whether it closed; `None` when it holds an invalid escape.
    fn string(&mut self) -> Option<(String, bool)> {
        let body = read_string_body(&self.bytes[self.pos + 1..])?;
        self.pos += 1 + body.consumed;
        Some((body.text, body.closed))
    }

    /// A number or literal, kept only when its end has streamed in.
    fn scalar(&mut self) -> Option<Parsed> {
        let start = self.pos;
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|byte| !matches!(byte, b',' | b'}' | b']') && !byte.is_ascii_whitespace())
        {
            self.pos += 1;
        }
        if self.pos == self.bytes.len() {
            return None;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        serde_json::from_str(text).ok().map(Parsed::complete)
    }
}

/// What `read_string_body` took from the start of a string body.
pub struct StringBody {
    pub text: String,
    /// Bytes of the input the text came from, including the closing quote; an escape cut off at
    /// the end is not consumed, so reading can resume at it once more has streamed in.
    pub consumed: usize,
    pub closed: bool,
}

/// Reads the JSON string whose opening quote has just been passed; `None` when it holds an
/// invalid escape.
pub fn read_string_body(raw: &[u8]) -> Option<StringBody> {
    let mut escaped = false;
    for (at, &byte) in raw.iter().enumerate() {
        match byte {
            _ if escaped => escaped = false,
            b'\\' => escaped = true,
            b'"' => {
                return Some(StringBody {
                    text: decode(&raw[..at])?,
                    consumed: at + 1,
                    closed: true,
                });
            }
            _ => {}
        }
    }
    let consumed = cut_escape_start(raw);
    Some(StringBody {
        text: decode(&raw[..consumed])?,
        consumed,
        closed: false,
    })
}

/// Decodes the body of a JSON string, the text between its quotes.
fn decode(body: &[u8]) -> Option<String> {
    let mut quoted = Vec::with_capacity(body.len() + 2);
    quoted.push(b'"');
    quoted.extend_from_slice(body);
    quoted.push(b'"');
    serde_json::from_slice(&quoted).ok()
}

/// Where an escape that `body` is cut off inside starts, or its length when it ends between
/// characters. A trailing `\uD800`-`\uDBFF` escape is cut off too, with the escape after it if
/// that is cut off: it is half of a pair.
fn cut_escape_start(body: &[u8]) -> usize {
    let mut at = 0;
    let mut high_surrogate_at = None;
    while at < body.len() {
        if body[at] != b'\\' {
            at += 1;
            high_surrogate_at = None;
            continue;
        }
        let escape_len = if body.get(at + 1) == Some(&b'u') { 6 } else { 2 };
        let end = at + escape_len;
        if end > body.len() {
            return high_surrogate_at.unwrap_or(at);
        }
        high_surrogate_at = is_high_surrogate(&body[at..end]).then_some(at);
        at = end;
    }
    high_surrogate_at.unwrap_or(body.len())
}

fn is_high_surrogate(escape: &[u8]) -> bool {
    escape.len() == 6
        && std::str::from_utf8(&escape[2..])
            .ok()
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
            .is_some_and(|unit| (0xD800..=0xDBFF).contains(&unit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_prefix_reads_as_the_arguments_streamed_so_far() {
        let cases = [
            ("", None),
            ("  ", None),
            ("{", Some(json!({}))),
            (r#"{"pa"#, Some(json!({}))),
            (r#"{"path""#, Some(json!({}))),
            (r#"{"path": "#, Some(json!({}))),
            (r#"{"path": "src/m"#, Some(json!({"path": "src/m"}))),
            (
                r#"{"path": "a.rs", "edits": ["#,
                Some(json!({"path": "a.rs", "edits": []})),
            ),
            (
                r#"{"path": "a.rs", "edits": [{"oldText": "fn a()\n    x"#,
                Some(json!({"path": "a.rs", "edits": [{"oldText": "fn a()\n    x"}]})),
            ),
            (
                r#"{"edits": [{"oldText": "a", "newText": "b"}, {"line": 4"#,
                Some(json!({"edits": [{"oldText": "a", "newText": "b"}, {}]})),
            ),
            (
                r#"{"edits": [{"line": 4, "ok": tru"#,
                Some(json!({"edits": [{"line": 4}]})),
            ),
            (
                r#"{"edits": [{"line": 4}], "dry": true}"#,
                Some(json!({"edits": [{"line": 4}], "dry": true})),
            ),
        ];
        for (prefix, expected) in cases {
            assert_eq!(parse_prefix(prefix), expected, "prefix {prefix:?}");
        }
    }

    #[test]
    fn an_escape_cut_off_at_the_end_is_dropped_until_it_completes() {
        let cases = [
            (r#"{"t": "a\"#, "a"),
            (r#"{"t": "a\\"#, "a\\"),
            (r#"{"t": "a\\\"#, "a\\"),
            (r#"{"t": "a\n"#, "a\n"),
            (r#"{"t": "a\u00"#, "a"),
            (r#"{"t": "a\u00e9"#, "a\u{e9}"),
            (r#"{"t": "a\\u00"#, "a\\u00"),
            (r#"{"t": "q\"x"#, "q\"x"),
            (r#"{"t": "a\ud83d"#, "a"),
            (r#"{"t": "a\ud83d\ude"#, "a"),
            (r#"{"t": "a\ud83d\"#, "a"),
            (r#"{"t": "a\ud83d\ude00"#, "a\u{1f600}"),
        ];
        for (prefix, text) in cases {
            assert_eq!(parse_prefix(prefix), Some(json!({"t": text})), "prefix {prefix:?}");
        }
    }

    #[test]
    fn every_prefix_of_a_call_parses_and_the_whole_call_reads_exactly() {
        let full = r#"{"path": "a.rs", "edits": [{"oldText": "é\n\"q\"", "newText": "x\ty", "line": 12}]}"#;
        for split in (0..=full.len()).filter(|&at| full.is_char_boundary(at)) {
            parse_prefix(&full[..split]);
        }
        assert_eq!(parse_prefix(full), serde_json::from_str(full).ok());
    }

    #[test]
    fn nesting_deeper_than_serde_json_allows_is_left_out() {
        let deep = format!("{{\"a\": {}", "[".repeat(100_000));
        let Some(Value::Object(parsed)) = parse_prefix(&deep) else {
            panic!("the outer object reads");
        };
        let mut depth = 0;
        let mut value = &parsed["a"];
        while let Some(inner) = value.as_array().and_then(|items| items.first()) {
            depth += 1;
            value = inner;
        }
        assert_eq!(depth, MAX_DEPTH - 2);
    }
}
