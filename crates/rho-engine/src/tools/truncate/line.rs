use super::GREP_MAX_LINE_LENGTH;

/// A single line passed through [`truncate_line`].
pub struct TruncatedLine {
    pub text: String,
    pub was_truncated: bool,
}

/// Truncate a single line to [`GREP_MAX_LINE_LENGTH`] chars, appending
/// `... [truncated]` (pi's `truncateLine`, used for search match lines).
pub fn truncate_line(line: &str) -> TruncatedLine {
    if line.chars().count() <= GREP_MAX_LINE_LENGTH {
        return TruncatedLine {
            text: line.to_string(),
            was_truncated: false,
        };
    }
    TruncatedLine {
        text: format!(
            "{}... [truncated]",
            line.chars().take(GREP_MAX_LINE_LENGTH).collect::<String>()
        ),
        was_truncated: true,
    }
}
