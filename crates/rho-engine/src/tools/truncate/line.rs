use super::GREP_MAX_LINE_LENGTH;

/// A single line passed through [`truncate_line`].
pub struct TruncatedLine {
    pub text: String,
    pub was_truncated: bool,
}

/// Truncate a single line to [`GREP_MAX_LINE_LENGTH`] chars, appending
/// `... [truncated]` (pi's `truncateLine`, used for search match lines).
pub fn truncate_line(line: &str) -> TruncatedLine {
    if line.len() <= GREP_MAX_LINE_LENGTH {
        return TruncatedLine {
            text: line.to_string(),
            was_truncated: false,
        };
    }
    match line.char_indices().nth(GREP_MAX_LINE_LENGTH) {
        None => TruncatedLine {
            text: line.to_string(),
            was_truncated: false,
        },
        Some((byte_idx, _)) => TruncatedLine {
            text: format!("{}... [truncated]", &line[..byte_idx]),
            was_truncated: true,
        },
    }
}
