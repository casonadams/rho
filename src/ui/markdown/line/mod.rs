//! Markdown line formatting for headers, list items, quotes, rules, and code fences.

mod blocks;
mod buffer;
mod fence;

use super::elements::render_inline_elements;
use super::highlight::highlight_code_line;
use crate::ui::theme::Theme;

pub use blocks::{render_header, render_horizontal_rule, render_list_item, render_quote};
pub use buffer::{needs_preceding_blank_line, should_buffer_line};
pub use fence::CodeFenceTracker;

pub fn render_line(line: &str, code_fence: &mut CodeFenceTracker, theme: &Theme) -> String {
    let trimmed = line.trim();

    if trimmed.starts_with("```") {
        return code_fence.toggle(trimmed, theme);
    }
    if code_fence.in_code_block {
        return highlight_code_line(line, code_fence.code_lang.as_deref(), theme);
    }
    if let Some(rule) = render_horizontal_rule(line, theme) {
        return rule;
    }
    if let Some(header) = render_header(line, theme) {
        return header;
    }
    if let Some(list_item) = render_list_item(line, theme) {
        return list_item;
    }
    if let Some(quote) = render_quote(line, theme) {
        return quote;
    }

    render_inline_elements(line, theme)
}
