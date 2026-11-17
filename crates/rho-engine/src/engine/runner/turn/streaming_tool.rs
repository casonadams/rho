use crate::engine::runner::sink::TerminalApprovalSink;
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallDelta {
    Name(String),
    Delta(String),
}

#[derive(Default)]
pub struct StreamingToolTracker {
    name: Option<String>,
    arguments_buf: String,
    path_started: bool,
    streamed_content_len: usize,
    content: ContentDecoder,
}

impl StreamingToolTracker {
    pub fn handle_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    pub fn handle_delta(&mut self, chunk: &str, sink: &Arc<TerminalApprovalSink>) {
        self.arguments_buf.push_str(chunk);
        if self.name.as_deref() == Some("write") {
            self.maybe_start_path(sink);
            self.maybe_stream_content(sink);
        }
    }

    #[cfg(test)]
    pub fn handle_tool_call_delta(&mut self, content: ToolCallDelta, sink: &Arc<TerminalApprovalSink>) {
        match content {
            ToolCallDelta::Name(name) => self.handle_name(name),
            ToolCallDelta::Delta(chunk) => self.handle_delta(&chunk, sink),
        }
    }

    fn maybe_start_path(&mut self, sink: &Arc<TerminalApprovalSink>) {
        if self.path_started {
            return;
        }
        if let Some(path) = self.extract_target_path() {
            self.path_started = true;
            sink.tool_start("write", &serde_json::json!({ "path": path }));
        }
    }

    fn maybe_stream_content(&mut self, sink: &Arc<TerminalApprovalSink>) {
        self.content.advance(&self.arguments_buf);
        let current_content = &self.content.decoded;
        if current_content.len() > self.streamed_content_len {
            sink.tool_chunk(&current_content[self.streamed_content_len..]);
            self.streamed_content_len = current_content.len();
        }
    }

    fn extract_target_path(&self) -> Option<String> {
        extract_json_string_field(&self.arguments_buf, "path")
            .or_else(|| extract_json_string_field(&self.arguments_buf, "file_path"))
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let start = locate_string_body(json, &format!("\"{field}\""))?;
    decode_closed_json_string(&json[start..])
}

fn locate_string_body(json: &str, field_key: &str) -> Option<usize> {
    let key_pos = json.find(field_key)?;
    let after_key = &json[key_pos + field_key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let string_body = after_colon.strip_prefix('"')?;
    Some(json.len() - string_body.len())
}

#[derive(Default)]
struct ContentDecoder {
    body_start: Option<usize>,
    scanned: usize,
    decoded: String,
    closed: bool,
}

impl ContentDecoder {
    fn advance(&mut self, json: &str) {
        if self.closed {
            return;
        }
        let start = match self.body_start {
            Some(start) => start,
            None => {
                let Some(start) = locate_string_body(json, "\"content\"") else {
                    return;
                };
                self.body_start = Some(start);
                start
            }
        };
        let pending = &json[start + self.scanned..];
        let mut pos = 0;
        while pos < pending.len() {
            let rest = &pending[pos..];
            let Some(special) = rest.find(['"', '\\']) else {
                self.decoded.push_str(rest);
                pos = pending.len();
                break;
            };
            self.decoded.push_str(&rest[..special]);
            let at = pos + special;
            if pending.as_bytes()[at] == b'"' {
                self.closed = true;
                pos = at + 1;
                break;
            }
            // Backslash: an escape split across deltas is left unconsumed until complete.
            let Some(next_ch) = pending[at + 1..].chars().next() else {
                pos = at;
                break;
            };
            append_escaped_char(&mut self.decoded, next_ch);
            pos = at + 1 + next_ch.len_utf8();
        }
        self.scanned += pos;
    }
}

fn decode_closed_json_string(string_content: &str) -> Option<String> {
    let mut result = String::new();
    let mut chars = string_content.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => append_escaped_char(&mut result, chars.next()?),
            '"' => return Some(result),
            c => result.push(c),
        }
    }
    None
}

const ESCAPE_CHARS: [(char, char); 8] = [
    ('"', '"'),
    ('\\', '\\'),
    ('/', '/'),
    ('b', '\x08'),
    ('f', '\x0c'),
    ('n', '\n'),
    ('r', '\r'),
    ('t', '\t'),
];

fn append_escaped_char(result: &mut String, next_ch: char) {
    if let Some(&(_, unescaped)) = ESCAPE_CHARS.iter().find(|&&(k, _)| k == next_ch) {
        result.push(unescaped);
    } else {
        result.push('\\');
        result.push(next_ch);
    }
}
