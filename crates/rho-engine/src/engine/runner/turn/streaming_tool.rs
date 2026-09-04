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
                    if !self.path_started
                        && let Some(path) = extract_json_string_field(&self.arguments_buf, "path")
                            .or_else(|| extract_json_string_field(&self.arguments_buf, "file_path"))
                    {
                        self.path_started = true;
                        sink.tool_start("write", &serde_json::json!({ "path": path }));
                    }
                    if let Some(current_content) = extract_json_streaming_content(&self.arguments_buf)
                        && current_content.len() > self.streamed_content_len
                    {
                        let delta = &current_content[self.streamed_content_len..];
                        sink.tool_chunk(delta);
                        self.streamed_content_len = current_content.len();
                    }
                }
            }
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let key_pos = json.find(&key)?;
    let after_key = &json[key_pos + key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let string_content = &after_colon[1..];
    let mut result = String::new();
    let mut chars = string_content.char_indices();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\\' => {
                if let Some((_, next_ch)) = chars.next() {
                    append_escaped_char(&mut result, next_ch);
                }
            }
            '"' => return Some(result),
            c => result.push(c),
        }
    }
    None
}

pub fn extract_json_streaming_content(json: &str) -> Option<String> {
    let key = "\"content\"";
    let key_pos = json.find(key)?;
    let after_key = &json[key_pos + key.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let string_content = &after_colon[1..];
    let mut result = String::new();
    let mut chars = string_content.char_indices();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\\' => {
                if let Some((_, next_ch)) = chars.next() {
                    append_escaped_char(&mut result, next_ch);
                } else {
                    break;
                }
            }
            '"' => return Some(result),
            c => result.push(c),
        }
    }
    Some(result)
}

fn append_escaped_char(result: &mut String, next_ch: char) {
    match next_ch {
        '"' => result.push('"'),
        '\\' => result.push('\\'),
        '/' => result.push('/'),
        'b' => result.push('\x08'),
        'f' => result.push('\x0c'),
        'n' => result.push('\n'),
        'r' => result.push('\r'),
        't' => result.push('\t'),
        other => {
            result.push('\\');
            result.push(other);
        }
    }
}
