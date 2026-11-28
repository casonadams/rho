//! Antigravity wire format, stream side: SSE chunk decoding into
//! canonical streaming events.

#[cfg(test)]
mod tests;

use crate::provider::google_wire::{GoogleSseParser, ThoughtReplay};

/// Antigravity replays thoughts only through the signature on the tool call that follows them.
pub fn sse_parser() -> GoogleSseParser {
    GoogleSseParser::new(ThoughtReplay::ToolCallSignature)
}
