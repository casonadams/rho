use super::runner::MultiTurnStreamItem;
use crate::engine::runner::sink::TerminalApprovalSink;
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct StreamingToolTracker {
    call_id: Option<String>,
    arguments_buf: String,
    preview_started: bool,
    path_shown: bool,
    streamed_content_len: usize,
    content: ContentDecoder,
}

impl StreamingToolTracker {
    /// Feeds a fragment of tool call `id`'s argument JSON; a new `id` starts a new call.
    pub fn handle_delta(&mut self, id: &str, name: &str, chunk: &str, sink: &Arc<TerminalApprovalSink>) {
        if self.call_id.as_deref() != Some(id) {
            *self = Self {
                call_id: Some(id.to_string()),
                ..Self::default()
            };
            sink.flush_display();
        }
        self.arguments_buf.push_str(chunk);
        if name == "write" {
            self.maybe_start_preview(sink);
            self.maybe_stream_content(sink);
        }
    }

    /// Opens the card on the first fragment, before the path is known.
    fn maybe_start_preview(&mut self, sink: &Arc<TerminalApprovalSink>) {
        if self.preview_started {
            return;
        }
        self.preview_started = true;
        let path = self.extract_target_path();
        self.path_shown = path.is_some();
        let args = match path {
            Some(path) => serde_json::json!({ "path": path }),
            None => serde_json::json!({}),
        };
        sink.preview_tool("write", &args);
    }

    /// Reopens the card once the path arrives, replaying the content so far, so the header and language are right.
    fn maybe_reopen_with_path(&mut self, sink: &Arc<TerminalApprovalSink>) {
        if self.path_shown {
            return;
        }
        let Some(path) = self.extract_target_path() else {
            return;
        };
        self.path_shown = true;
        sink.preview_tool("write", &serde_json::json!({ "path": path }));
        self.streamed_content_len = 0;
    }

    fn maybe_stream_content(&mut self, sink: &Arc<TerminalApprovalSink>) {
        if !self.preview_started {
            return;
        }
        self.content.advance(&self.arguments_buf);
        self.maybe_reopen_with_path(sink);
        let current_content = &self.content.decoded;
        if current_content.len() > self.streamed_content_len {
            sink.preview_tool_chunk(&current_content[self.streamed_content_len..]);
            self.streamed_content_len = current_content.len();
        }
    }

    fn extract_target_path(&self) -> Option<String> {
        rho_harness_core::args::TARGET_PATH_KEYS
            .iter()
            .find_map(|key| extract_json_string_field(&self.arguments_buf, key))
    }

    /// Ends the call being streamed when the stream moves past it: its model call finished or was
    /// retried, or its tools were committed to run. A retried call's preview card is removed.
    pub fn observe_boundary(&mut self, item: &MultiTurnStreamItem, sink: &Arc<TerminalApprovalSink>) {
        match item {
            MultiTurnStreamItem::FinalResponse(_)
            | MultiTurnStreamItem::CompletionCall(_)
            | MultiTurnStreamItem::ToolExecutionCommitted => self.reset(),
            MultiTurnStreamItem::ModelTurnRetried(_) => {
                self.reset();
                sink.cancel_tool_preview();
            }
            _ => {}
        }
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
                let Some(start) =
                    locate_string_body(json, "\"text\"").or_else(|| locate_string_body(json, "\"content\""))
                else {
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
