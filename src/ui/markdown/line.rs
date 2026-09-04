//! Markdown line formatting for headers, list items, quotes, and code fences.

use super::elements::render_inline_elements;
use super::highlight::highlight_code_line;
use crate::ui::theme::Theme;
use std::sync::LazyLock;

static ORDERED_LIST: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(\d+\.)\s+(.*)$").expect("valid ordered list pattern"));

#[derive(Default)]
pub struct CodeFenceTracker {
    pub in_code_block: bool,
    pub code_lang: Option<String>,
}

impl CodeFenceTracker {
    pub fn toggle(&mut self, trimmed: &str, theme: &Theme) -> String {
        let tag = trimmed.trim_start_matches('`').trim();
        let dim = theme.dimmed;
        if self.in_code_block {
            self.in_code_block = false;
            self.code_lang = None;
            format!("{dim}```{dim:#}")
        } else {
            self.in_code_block = true;
            self.code_lang = (!tag.is_empty()).then(|| tag.to_string());
            format!("{dim}```{tag}{dim:#}")
        }
    }
}

pub fn render_header(line: &str, theme: &Theme) -> Option<String> {
    if let Some(rest) = line.strip_prefix("### ") {
        let h = theme.heading_h3;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{h}### {formatted}{h:#}"));
    }
    if let Some(rest) = line.strip_prefix("## ") {
        let p = theme.heading_h2;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{p}## {formatted}{p:#}"));
    }
    if let Some(rest) = line.strip_prefix("# ") {
        let hl = theme.heading_h1;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{hl}# {formatted}{hl:#}"));
    }
    None
}

pub fn render_list_item(line: &str, theme: &Theme) -> Option<String> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        let p = theme.prompt;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{p}•{p:#} {formatted}"));
    }
    if let Some(caps) = ORDERED_LIST.captures(line) {
        let num = &caps[1];
        let rest = &caps[2];
        let p = theme.prompt;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{p}{num}{p:#} {formatted}"));
    }
    None
}

pub fn render_line(line: &str, code_fence: &mut CodeFenceTracker, theme: &Theme) -> String {
    let trimmed = line.trim();

    if trimmed.starts_with("```") {
        return code_fence.toggle(trimmed, theme);
    }
    if code_fence.in_code_block {
        return highlight_code_line(line, code_fence.code_lang.as_deref(), theme);
    }
    if let Some(header) = render_header(line, theme) {
        return header;
    }
    if let Some(list_item) = render_list_item(line, theme) {
        return list_item;
    }
    if let Some(quote) = line.strip_prefix("> ") {
        let d = theme.dimmed;
        let formatted = render_inline_elements(quote, theme);
        return format!("{d}│{d:#} {formatted}");
    }

    render_inline_elements(line, theme)
}

pub fn should_buffer_line(current_line: &str) -> bool {
    let trimmed = current_line.trim_start();
    trimmed.starts_with('|')
        || trimmed.starts_with('#')
        || trimmed.starts_with('`')
        || trimmed == ">"
        || trimmed.starts_with("> ")
        || trimmed == "-"
        || trimmed.starts_with("- ")
        || trimmed == "*"
        || trimmed.starts_with("* ")
        || trimmed.chars().all(|character| character.is_ascii_digit())
        || (trimmed.len() >= 2 && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) && trimmed.contains(". "))
}

pub fn needs_preceding_blank_line(trimmed: &str, in_code_block: bool) -> bool {
    trimmed.starts_with('#') || (trimmed.starts_with("```") && !in_code_block) || trimmed.starts_with('>')
}
