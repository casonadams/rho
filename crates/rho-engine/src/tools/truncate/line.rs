use std::borrow::Cow;

use super::GREP_MAX_LINE_LENGTH;

/// A single line passed through [`truncate_line`].
pub struct TruncatedLine<'a> {
    pub text: Cow<'a, str>,
    pub was_truncated: bool,
}

/// Truncate a single line to [`GREP_MAX_LINE_LENGTH`] chars, appending
/// `... [truncated]` (pi's `truncateLine`, used for search match lines).
pub fn truncate_line(line: &str) -> TruncatedLine<'_> {
    if line.len() <= GREP_MAX_LINE_LENGTH {
        return TruncatedLine {
            text: Cow::Borrowed(line),
            was_truncated: false,
        };
    }
    match line.char_indices().nth(GREP_MAX_LINE_LENGTH) {
        None => TruncatedLine {
            text: Cow::Borrowed(line),
            was_truncated: false,
        },
        Some((byte_idx, _)) => TruncatedLine {
            text: Cow::Owned(format!("{}... [truncated]", &line[..byte_idx])),
            was_truncated: true,
        },
    }
}
