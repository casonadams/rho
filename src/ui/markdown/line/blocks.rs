//! Block-level element rendering (headers, lists, task items, quotes, and rules).

use crate::ui::markdown::elements::render_inline_elements;
use crate::ui::theme::Theme;
use std::sync::LazyLock;

static ORDERED_LIST: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^(\d+\.)\s+(.*)$").expect("valid ordered list pattern"));

pub fn render_header(line: &str, theme: &Theme) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, trimmed) = line.split_at(indent_len);
    if let Some(rest) = trimmed
        .strip_prefix("###### ")
        .or_else(|| trimmed.strip_prefix("##### "))
        .or_else(|| trimmed.strip_prefix("#### "))
        .or_else(|| trimmed.strip_prefix("### "))
    {
        let h = theme.heading_h3;
        let prefix_len = trimmed.len() - rest.len();
        let hashes = &trimmed[..prefix_len];
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{indent}{h}{hashes}{formatted}{h:#}"));
    }
    if let Some(rest) = trimmed.strip_prefix("## ") {
        let p = theme.heading_h2;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{indent}{p}## {formatted}{p:#}"));
    }
    if let Some(rest) = trimmed.strip_prefix("# ") {
        let hl = theme.heading_h1;
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{indent}{hl}# {formatted}{hl:#}"));
    }
    None
}

pub fn render_list_item(line: &str, theme: &Theme) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, trimmed) = line.split_at(indent_len);

    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        return Some(render_bullet_or_task(indent, rest, theme));
    }
    if let Some(caps) = ORDERED_LIST.captures(trimmed) {
        let num = &caps[1];
        let rest = &caps[2];
        let p = theme.prompt;
        if let Some((box_str, item_text)) = parse_task_checkbox(rest, theme) {
            let formatted = render_inline_elements(item_text, theme);
            return Some(format!("{indent}{p}{num}{p:#} {box_str} {formatted}"));
        }
        let formatted = render_inline_elements(rest, theme);
        return Some(format!("{indent}{p}{num}{p:#} {formatted}"));
    }
    None
}

fn parse_task_checkbox<'a>(text: &'a str, theme: &Theme) -> Option<(String, &'a str)> {
    if let Some(rest) = text.strip_prefix("[ ] ") {
        let d = theme.dimmed;
        Some((format!("{d}[ ]{d:#}"), rest))
    } else if let Some(rest) = text.strip_prefix("[x] ").or_else(|| text.strip_prefix("[X] ")) {
        let p = theme.prompt;
        Some((format!("{p}[x]{p:#}"), rest))
    } else {
        None
    }
}

fn render_bullet_or_task(indent: &str, rest: &str, theme: &Theme) -> String {
    if let Some((box_str, item_text)) = parse_task_checkbox(rest, theme) {
        let formatted = render_inline_elements(item_text, theme);
        format!("{indent}{box_str} {formatted}")
    } else {
        let p = theme.prompt;
        let formatted = render_inline_elements(rest, theme);
        format!("{indent}{p}•{p:#} {formatted}")
    }
}

pub fn render_quote(line: &str, theme: &Theme) -> Option<String> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, mut trimmed) = line.split_at(indent_len);
    if !trimmed.starts_with('>') {
        return None;
    }

    let mut bars = 0;
    while let Some(rest) = trimmed.strip_prefix('>') {
        bars += 1;
        trimmed = rest.trim_start();
    }

    let d = theme.dimmed;
    let quote_prefix = format!("{d}│{d:#} ").repeat(bars);
    if trimmed.is_empty() {
        Some(format!("{indent}{}", quote_prefix.trim_end()))
    } else {
        let formatted = render_inline_elements(trimmed, theme);
        Some(format!("{indent}{quote_prefix}{formatted}"))
    }
}

pub fn is_horizontal_rule(trimmed: &str) -> bool {
    let stripped: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if stripped.len() < 3 {
        return false;
    }
    let first = stripped.chars().next().unwrap_or(' ');
    matches!(first, '-' | '*' | '_') && stripped.chars().all(|c| c == first)
}

pub fn render_horizontal_rule(line: &str, theme: &Theme) -> Option<String> {
    let trimmed = line.trim();
    if is_horizontal_rule(trimmed) {
        let width = crossterm::terminal::size()
            .map(|(cols, _)| usize::from(cols).max(40))
            .unwrap_or(80);
        let d = theme.dimmed;
        let rule = "─".repeat(width);
        return Some(format!("{d}{rule}{d:#}"));
    }
    None
}
