use crate::engine::runner::sink::TerminalApprovalSink;
use rig::streaming::ToolCallDeltaContent;
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub struct StreamingToolTracker {
    name: Option<String>,
    arguments_buf: String,
    path_started: bool,
    streamed_content_len: usize,
}

impl StreamingToolTracker {
    pub fn handle_delta(&mut self, content: ToolCallDeltaContent, sink: &Arc<TerminalApprovalSink>) {
        match content {
            ToolCallDeltaContent::Name(name) => {
                self.name = Some(name);
            }
            ToolCallDeltaContent::Delta(chunk) => {
                self.arguments_buf.push_str(&chunk);
                if self.name.as_deref() == Some("write") {
                    self.maybe_start_path(sink);
                    self.maybe_stream_content(sink);
                }
            }
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
        if let Some(current_content) = extract_json_streaming_content(&self.arguments_buf)
            && current_content.len() > self.streamed_content_len
        {
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
    let key = format!("\"{field}\"");
    parse_json_string_value(json, &key, false)
}

pub fn extract_json_streaming_content(json: &str) -> Option<String> {
    parse_json_string_value(json, "\"content\"", true)
}

fn parse_json_string_value(json: &str, field_key: &str, is_streaming: bool) -> Option<String> {
    let key_pos = json.find(field_key)?;
    let after_key = &json[key_pos + field_key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    let string_content = after_colon.strip_prefix('"')?;

    let (result, closed) = decode_json_string_content(string_content, is_streaming);
    if closed || is_streaming { Some(result) } else { None }
}

fn decode_json_string_content(string_content: &str, is_streaming: bool) -> (String, bool) {
    let mut result = String::new();
    let mut chars = string_content.char_indices();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\\' => {
                if let Some((_, next_ch)) = chars.next() {
                    append_escaped_char(&mut result, next_ch);
                } else if is_streaming {
                    break;
                }
            }
            '"' => return (result, true),
            c => result.push(c),
        }
    }
    (result, false)
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
