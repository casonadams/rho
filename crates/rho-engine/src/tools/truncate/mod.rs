//! Shared truncation utilities for tool outputs, ported from pi's
//! `truncate.ts`. Two independent limits apply - whichever is hit first wins:
//! a line limit (default 2000) and a byte limit (default 50KB). Neither
//! function returns partial lines; head truncation reports an oversized first
//! line through `first_line_exceeds_limit` and tail truncation reports a
//! partial final line through `last_line_partial`.

mod head;
mod line;
mod tail;

pub use head::truncate_head;
pub use line::{TruncatedLine, truncate_line};
pub use tail::truncate_tail;

#[cfg(test)]
mod tests;

pub const DEFAULT_MAX_LINES: usize = 2000;
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024; // 50 KB
/// Max chars per search match line (pi's `GREP_MAX_LINE_LENGTH`).
pub const GREP_MAX_LINE_LENGTH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncatedBy {
    Lines,
    Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    pub content: String,
    pub truncated: bool,
    pub truncated_by: Option<TruncatedBy>,
    pub total_lines: usize,
    pub total_bytes: usize,
    pub output_lines: usize,
    pub output_bytes: usize,
    /// Tail edge case: the first kept line was partially truncated from the end.
    pub last_line_partial: bool,
    /// Head edge case: the first line alone exceeded the byte limit, so no
    /// content was emitted.
    pub first_line_exceeds_limit: bool,
    pub max_lines: usize,
    pub max_bytes: usize,
}

/// Format bytes as human-readable size (pi's `formatSize`).
pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
