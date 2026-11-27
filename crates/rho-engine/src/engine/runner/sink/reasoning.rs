//! Reasoning stream chunk splitting and spacing normalization.

use super::types::{DisplayKind, TerminalSinkState};

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

pub(super) struct ReasoningStreamPlan {
    pub(super) prefix_blank: bool,
    pub(super) internal_newlines: Option<&'static str>,
    pub(super) content: String,
}

fn pop_internal_newlines(state: &mut TerminalSinkState) -> Option<&'static str> {
    if !state.has_reasoning_content || state.pending_reasoning_newlines == 0 {
        return None;
    }
    let count = state.pending_reasoning_newlines.min(2);
    state.pending_reasoning_newlines = 0;
    Some(if count == 1 { "\n" } else { "\n\n" })
}

pub(super) fn update_reasoning_state(state: &mut TerminalSinkState, text_to_stream: &str) -> ReasoningStreamPlan {
    state.reasoning.push(text_to_stream.to_string());
    let (content, trailing_newlines) = split_reasoning_chunk(text_to_stream);
    if content.is_empty() {
        state.pending_reasoning_newlines += trailing_newlines;
        return ReasoningStreamPlan {
            prefix_blank: false,
            internal_newlines: None,
            content: String::new(),
        };
    }
    let internal_newlines = pop_internal_newlines(state);
    let prefix_blank = matches!(
        state.last_display,
        DisplayKind::Tool | DisplayKind::Text | DisplayKind::None
    );
    state.last_display = DisplayKind::Thinking;
    state.has_reasoning_content = true;
    state.pending_reasoning_newlines = trailing_newlines;
    ReasoningStreamPlan {
        prefix_blank,
        internal_newlines,
        content: content.to_string(),
    }
}
