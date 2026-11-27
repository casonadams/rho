//! Edit-diff, write-preview, session-status, and relative-time formatters.
//!
//! These are `pub(crate)` because they are only consumed by `renderer.rs`,
//! but they remain exposed as module-private items so future tools can reuse them.

use super::file_change;
use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use rho_harness_core::presentation::FileChange;
use rho_harness_core::presentation::SessionStatus;
use std::fmt::Write;

pub(crate) struct EditDiffRequest<'a> {
    pub args: &'a serde_json::Value,
    pub file_change: Option<&'a FileChange>,
    pub expanded: bool,
    pub width: usize,
}

/// The collapsed edit card keeps its newest `PREVIEW_LINES` rows, like the write card, and clips each to one row.
pub(crate) fn format_edit_diff(request: EditDiffRequest<'_>, theme: &Theme) -> Option<String> {
    let edits = request.args.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }
    let start_lines = match request.file_change {
        Some(change) => change.edit_start_lines.clone(),
        None => {
            let needs_file = edits.iter().any(|edit| file_change::explicit_edit_line(edit).is_none());
            let content = rho_harness_core::args::target_path(request.args)
                .filter(|_| needs_file)
                .and_then(file_change::read_bounded);
            file_change::edit_start_lines(edits, content.as_deref())
        }
    };
    let lang = super::card::detect_language_from_args(request.args);
    let mut out = String::new();
    for (idx, edit) in edits.iter().enumerate() {
        let old_text = file_change::removed_text(edit, idx, request.file_change);
        out.push_str(&super::diff::format_entry_diff(super::diff::EntryDiffInput {
            idx,
            total: edits.len(),
            old_text: &old_text,
            new_text: edit.get("newText").and_then(|v| v.as_str()).unwrap_or(""),
            theme,
            start_line: start_lines.get(idx).copied().flatten(),
            lang,
        }));
    }
    if request.expanded {
        return Some(out);
    }
    Some(collapse_to_tail(&out, request.width, theme))
}

fn collapse_to_tail(rendered: &str, width: usize, theme: &Theme) -> String {
    let rows: Vec<&str> = rendered.lines().collect();
    let hidden = rows.len().saturating_sub(PREVIEW_LINES);
    let mut out = earlier_lines_notice(hidden, rows.len(), width, theme);
    for row in &rows[hidden..] {
        let _ = writeln!(out, "{}", crate::ui::interactive::footer::truncate_to_width(row, width));
    }
    out
}

fn earlier_lines_notice(hidden: usize, total: usize, width: usize, theme: &Theme) -> String {
    if hidden == 0 {
        return String::new();
    }
    let dim = theme.dimmed;
    let notice = format!("... ({hidden} earlier lines, {total} total, Ctrl+O to expand)");
    let notice = crate::ui::interactive::footer::truncate_to_width(&notice, width);
    format!("{dim}{notice}{dim:#}\n")
}

/// Highlights every line so highlighter state carries over, but emits only lines from `first` on.
/// With `clip` set each line is cut to that many columns so it takes exactly one row.
fn format_preview_lines(
    lines: &[&str],
    first: usize,
    clip: Option<usize>,
    lang: Option<&str>,
    theme: &Theme,
) -> String {
    let mut out = String::new();
    let d = theme.dimmed;
    let gutter_width = lines.len().to_string().len().max(3);
    let mut highlighter = crate::ui::markdown::CodeHighlighter::new(lang);
    for (idx, line) in lines.iter().enumerate() {
        let no_tabs = super::diff::replace_tabs(line);
        let highlighted = highlighter.highlight_line(&no_tabs, theme);
        if idx >= first {
            let gutter = super::diff::format_gutter_prefix(idx + 1, gutter_width, d);
            let row = format!("{gutter}{highlighted}");
            let row = match clip {
                Some(width) => crate::ui::interactive::footer::truncate_to_width(&row, width),
                None => row,
            };
            let _ = writeln!(out, "{row}");
        }
    }
    out
}

struct ParsedReadLine<'a> {
    num: Option<usize>,
    content: &'a str,
    is_notice: bool,
}

fn parse_tabbed_read_lines<'a>(lines: &[&'a str]) -> Vec<ParsedReadLine<'a>> {
    let mut parsed = Vec::with_capacity(lines.len());
    for line in lines {
        if let Some((num, code)) = parse_read_line(line) {
            parsed.push(ParsedReadLine {
                num: Some(num),
                content: code,
                is_notice: false,
            });
        } else {
            let trimmed = line.trim();
            let is_notice = trimmed.starts_with('[') && trimmed.ends_with(']');
            parsed.push(ParsedReadLine {
                num: None,
                content: line,
                is_notice,
            });
        }
    }
    parsed
}

fn parse_offset_read_lines<'a>(lines: &[&'a str], clean: &'a str, offset: usize) -> Vec<ParsedReadLine<'a>> {
    let trimmed = clean.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.contains('\n') {
        vec![ParsedReadLine {
            num: None,
            content: clean,
            is_notice: true,
        }]
    } else {
        lines
            .iter()
            .enumerate()
            .map(|(idx, line)| ParsedReadLine {
                num: Some(offset + idx),
                content: line,
                is_notice: false,
            })
            .collect()
    }
}

fn parse_read_lines<'a>(lines: &[&'a str], clean: &'a str, offset: usize) -> Vec<ParsedReadLine<'a>> {
    if lines.iter().any(|l| parse_read_line(l).is_some()) {
        parse_tabbed_read_lines(lines)
    } else {
        parse_offset_read_lines(lines, clean, offset)
    }
}

fn render_parsed_read_lines(
    parsed: Vec<ParsedReadLine>,
    highlighter: &mut crate::ui::markdown::CodeHighlighter,
    theme: &Theme,
) -> String {
    let max_line = parsed.iter().filter_map(|p| p.num).max().unwrap_or(1);
    let gutter_width = max_line.to_string().len().max(3);

    let mut out = String::new();
    let d = theme.dimmed;
    for item in parsed {
        if let Some(num) = item.num {
            let no_tabs = super::diff::replace_tabs(item.content);
            let highlighted = highlighter.highlight_line(&no_tabs, theme);
            let gutter = super::diff::format_gutter_prefix(num, gutter_width, d);
            let _ = writeln!(out, "{gutter}{highlighted}");
        } else if item.is_notice {
            let _ = writeln!(out, "{d}{}{d:#}", item.content);
        } else {
            out.push_str(item.content);
            out.push('\n');
        }
    }
    out
}

pub(crate) fn format_read_expanded(raw: &str, args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let clean = raw.trim_end();
    if clean.is_empty() {
        return None;
    }
    let lang = super::card::detect_language_from_args(args);
    let mut highlighter = crate::ui::markdown::CodeHighlighter::new(lang);
    let lines: Vec<&str> = clean.lines().collect();
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

    let parsed = parse_read_lines(&lines, clean, offset);
    Some(render_parsed_read_lines(parsed, &mut highlighter, theme))
}

fn parse_read_line(line: &str) -> Option<(usize, &str)> {
    let (prefix, rest) = line.split_once('\t')?;
    let num = prefix.trim().parse::<usize>().ok()?;
    Some((num, rest))
}

/// The collapsed preview grows with the file up to `PREVIEW_LINES` rows, then adds a status row; it never shrinks.
pub(crate) fn format_write_preview(
    args: &serde_json::Value,
    theme: &Theme,
    expanded: bool,
    width: usize,
) -> Option<String> {
    let content = rho_harness_core::args::write_text(args)?;
    if content.trim().is_empty() {
        return None;
    }
    let lang = super::card::detect_language_from_args(args);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if expanded {
        return Some(format_preview_lines(&lines, 0, None, lang, theme));
    }
    let hidden = total.saturating_sub(PREVIEW_LINES);
    let mut out = earlier_lines_notice(hidden, total, width, theme);
    out.push_str(&format_preview_lines(&lines, hidden, Some(width), lang, theme));
    Some(out)
}

const PREVIEW_LINES: usize = 10;

pub(crate) fn format_relative_time(time: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(time);
    let secs = diff.num_seconds();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 2592000 {
        format!("{}d ago", secs / 86400)
    } else {
        time.format("%Y-%m-%d").to_string()
    }
}

pub fn format_session_status(session: &SessionStatus) -> String {
    match session.quota.as_deref() {
        Some(quota) => format!("{} | {} | {quota}", session.model, session.context),
        None => format!("{} | {}", session.model, session.context),
    }
}
