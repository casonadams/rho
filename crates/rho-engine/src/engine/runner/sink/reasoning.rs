//! Reasoning stream chunk splitting and spacing normalization.

#[cfg(test)]
mod tests;

/// Splits incoming reasoning text into renderable content and trailing newlines.
///
/// If the text ends with whitespace containing at least one newline, that trailing
/// whitespace is trimmed from the content and the newline count is returned.
/// Trailing whitespace without newlines (e.g. spaces between words) is preserved.
pub(crate) fn split_reasoning_chunk(text: &str) -> (&str, usize) {
    let trimmed = text.trim_end();
    let tail = &text[trimmed.len()..];
    let newline_count = tail.chars().filter(|&c| c == '\n').count();
    if newline_count > 0 {
        (trimmed, newline_count)
    } else {
        (text, 0)
    }
}
