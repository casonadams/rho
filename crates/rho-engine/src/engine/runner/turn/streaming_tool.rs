use super::runner::MultiTurnStreamItem;
use crate::engine::runner::sink::TerminalApprovalSink;
use crate::provider::json_nesting::JsonNesting;
use serde_json::{Value, json};
use std::sync::Arc;

mod partial_json;
#[cfg(test)]
pub(crate) mod tests;

/// Shows the card of a `write` or `edit` call while the model is still writing its arguments.
#[derive(Default)]
pub struct StreamingToolTracker {
    call_id: Option<String>,
    arguments_buf: String,
    preview: Option<Preview>,
}

enum Preview {
    Write(WritePreview),
    Edit(EditPreview),
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
        let buf = self.arguments_buf.as_str();
        match (&mut self.preview, name) {
            (Some(Preview::Write(preview)), _) => preview.advance(buf, sink),
            (Some(Preview::Edit(preview)), _) => preview.advance(buf, chunk, sink),
            (None, "write") => {
                let mut preview = WritePreview::open(id, buf, sink);
                preview.advance(buf, sink);
                self.preview = Some(Preview::Write(preview));
            }
            (None, "edit") => self.preview = Some(Preview::Edit(EditPreview::open(id, buf, sink))),
            (None, _) => {}
        }
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

fn extract_target_path(arguments: &str) -> Option<String> {
    rho_harness_core::args::TARGET_PATH_KEYS
        .iter()
        .find_map(|key| extract_json_string_field(arguments, key))
}

/// Finds what `extract_target_path` finds in arguments that only grow, searching each byte for
/// the keys once.
#[derive(Default)]
struct TargetPathScan {
    searched: usize,
    key_at: [Option<usize>; rho_harness_core::args::TARGET_PATH_KEYS.len()],
}

impl TargetPathScan {
    fn find(&mut self, arguments: &str) -> Option<String> {
        let keys = rho_harness_core::args::TARGET_PATH_KEYS;
        let from = arguments.floor_char_boundary(self.searched);
        for (key, at) in keys.iter().zip(&mut self.key_at) {
            if at.is_none() {
                *at = arguments[from..].find(&format!("\"{key}\"")).map(|found| from + found);
            }
        }
        let longest_key = keys.iter().map(|key| key.len() + 2).max().unwrap_or(0);
        self.searched = arguments.len().saturating_sub(longest_key - 1);
        keys.iter().zip(self.key_at).find_map(|(key, at)| {
            let start = at? + key.len() + 2;
            string_field_after_key(arguments, start)
        })
    }
}

fn path_arguments(path: Option<&str>) -> Value {
    match path {
        Some(path) => json!({ "path": path }),
        None => json!({}),
    }
}

/// A `write` card: opened before the path is known, its content streamed in as the card's output.
#[derive(Default)]
struct WritePreview {
    call_id: String,
    path_shown: bool,
    path_scan: TargetPathScan,
    streamed_content_len: usize,
    content: ContentDecoder,
}

impl WritePreview {
    /// Opens the card on the first fragment, before the path is known.
    fn open(call_id: &str, arguments: &str, sink: &Arc<TerminalApprovalSink>) -> Self {
        let path = extract_target_path(arguments);
        sink.preview_tool(call_id, "write", &path_arguments(path.as_deref()));
        Self {
            call_id: call_id.to_string(),
            path_shown: path.is_some(),
            ..Self::default()
        }
    }

    fn advance(&mut self, arguments: &str, sink: &Arc<TerminalApprovalSink>) {
        self.content.advance(arguments);
        self.reopen_with_path(arguments, sink);
        let current_content = &self.content.decoded;
        if current_content.len() > self.streamed_content_len {
            sink.preview_tool_chunk(&current_content[self.streamed_content_len..]);
            self.streamed_content_len = current_content.len();
        }
    }

    /// Reopens the card once the path arrives, replaying the content so far, so the header and language are right.
    fn reopen_with_path(&mut self, arguments: &str, sink: &Arc<TerminalApprovalSink>) {
        if self.path_shown {
            return;
        }
        let Some(path) = self.path_scan.find(arguments) else {
            return;
        };
        self.path_shown = true;
        sink.preview_tool(&self.call_id, "write", &path_arguments(Some(&path)));
        self.streamed_content_len = 0;
    }
}

/// An `edit` card, redrawn from the arguments streamed so far as they grow.
struct EditPreview {
    call_id: String,
    shown: Value,
    nesting: JsonNesting,
    read_len: usize,
}

/// The depth an edit object closes back to: inside the arguments object and its `edits` array.
const EDIT_CLOSED_DEPTH: usize = 2;
const MAX_UNREAD_BYTES: usize = 4096;

impl EditPreview {
    fn open(call_id: &str, arguments: &str, sink: &Arc<TerminalApprovalSink>) -> Self {
        let shown = Self::read(arguments).unwrap_or_else(|| json!({}));
        sink.preview_tool(call_id, "edit", &shown);
        let mut nesting = JsonNesting::default();
        nesting.feed(arguments);
        Self {
            call_id: call_id.to_string(),
            shown,
            nesting,
            read_len: arguments.len(),
        }
    }

    /// Rereads the arguments when an edit closes, or when `chunk` may end a line or a field and an
    /// eighth of the call, or 4 KiB, has streamed since the last reread.
    fn advance(&mut self, arguments: &str, chunk: &str, sink: &Arc<TerminalApprovalSink>) {
        let edit_closed = self.nesting.feed(chunk).is_some_and(|depth| depth <= EDIT_CLOSED_DEPTH);
        let unread = arguments.len() - self.read_len;
        let worth_rereading = unread >= MAX_UNREAD_BYTES.min(arguments.len() / 8);
        if !edit_closed && !(worth_rereading && chunk.contains(['\\', '"', ']', '}'])) {
            return;
        }
        self.read_len = arguments.len();
        let Some(parsed) = Self::read(arguments) else {
            return;
        };
        if parsed != self.shown {
            sink.preview_tool(&self.call_id, "edit", &parsed);
            self.shown = parsed;
        }
    }

    /// The arguments streamed so far, without a path still being written, which names no file yet.
    fn read(arguments: &str) -> Option<Value> {
        let mut parsed = partial_json::parse_prefix(arguments)?;
        if extract_target_path(arguments).is_none()
            && let Some(object) = parsed.as_object_mut()
        {
            for key in rho_harness_core::args::TARGET_PATH_KEYS.iter() {
                object.shift_remove(*key);
            }
        }
        Some(parsed)
    }
}

fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let field_key = format!("\"{field}\"");
    string_field_after_key(json, json.find(&field_key)? + field_key.len())
}

/// The closed string value of the key that ends at `after_key`.
fn string_field_after_key(json: &str, after_key: usize) -> Option<String> {
    let start = string_body_after_key(json, after_key)?;
    partial_json::read_string_body(&json.as_bytes()[start..])
        .filter(|body| body.closed)
        .map(|body| body.text)
}

fn locate_string_body(json: &str, field_key: &str) -> Option<usize> {
    string_body_after_key(json, json.find(field_key)? + field_key.len())
}

fn string_body_after_key(json: &str, after_key: usize) -> Option<usize> {
    let after_key = &json[after_key..];
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
        let Some(body) = partial_json::read_string_body(&json.as_bytes()[start + self.scanned..]) else {
            self.closed = true;
            return;
        };
        self.decoded.push_str(&body.text);
        self.scanned += body.consumed;
        self.closed = body.closed;
    }
}
