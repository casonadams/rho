//! Edit-diff, write-preview, thinking-block, session-status, and relative-time formatters.
//!
//! These are `pub(crate)` because they are only consumed by `renderer.rs`,
//! but they remain exposed as module-private items so future tools can reuse them.

use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use rho_harness_core::presentation::SessionStatus;

pub(crate) fn format_edit_diff(args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let edits = args.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }
    let path_str = args.get("path").and_then(|v| v.as_str());
    let mut out = String::new();
    for (idx, edit) in edits.iter().enumerate() {
        let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
        let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
        let start_line = edit
            .get("line")
            .or_else(|| edit.get("start_line"))
            .or_else(|| edit.get("line_number"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .or_else(|| path_str.and_then(|p| super::diff::find_edit_line_number(p, old_text, new_text)));

        out.push_str(&super::diff::format_entry_diff(super::diff::EntryDiffInput {
            idx,
            old_text,
            new_text,
            theme,
            start_line,
        }));
    }
    Some(out)
}

fn format_preview_lines(lines: &[&str], lang: Option<&str>, gutter_width: usize, theme: &Theme) -> String {
    let mut out = String::new();
    let d = theme.dimmed;
    let mut highlighter = crate::ui::markdown::CodeHighlighter::new(lang, theme);
    for (idx, line) in lines.iter().enumerate() {
        let line_num = idx + 1;
        let no_tabs = line.replace('\t', "   ");
        let highlighted = highlighter.highlight_line(&no_tabs, theme);
        let gutter = super::diff::format_gutter_prefix(line_num, gutter_width, d);
        out.push_str(&format!("{gutter}{highlighted}\n"));
    }
    out
}

pub(crate) fn format_read_expanded(raw: &str, args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let clean = raw.trim_end();
    if clean.is_empty() {
        return None;
    }
    let lang = super::preview::detect_language_from_args(args);
    let mut highlighter = crate::ui::markdown::CodeHighlighter::new(lang, theme);
    let lines: Vec<&str> = clean.lines().collect();

    let has_tab_numbering = lines.iter().any(|l| parse_read_line(l).is_some());
    let mut parsed: Vec<(Option<usize>, &str, bool)> = Vec::with_capacity(lines.len());
    if has_tab_numbering {
        for line in &lines {
            if let Some((num, code)) = parse_read_line(line) {
                parsed.push((Some(num), code, false));
            } else {
                let trimmed = line.trim();
                let is_notice = trimmed.starts_with('[') && trimmed.ends_with(']');
                parsed.push((None, line, is_notice));
            }
        }
    } else {
        let trimmed = clean.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.contains('\n') {
            parsed.push((None, clean, true));
        } else {
            let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            for (idx, line) in lines.iter().enumerate() {
                parsed.push((Some(offset + idx), line, false));
            }
        }
    }

    let max_line = parsed.iter().filter_map(|(num, _, _)| *num).max().unwrap_or(1);
    let gutter_width = max_line.to_string().len().max(3);

    let mut out = String::new();
    let d = theme.dimmed;
    for (num_opt, content, is_notice) in parsed {
        if let Some(num) = num_opt {
            let no_tabs = content.replace('\t', "   ");
            let highlighted = highlighter.highlight_line(&no_tabs, theme);
            let gutter = super::diff::format_gutter_prefix(num, gutter_width, d);
            out.push_str(&format!("{gutter}{highlighted}\n"));
        } else if is_notice {
            out.push_str(&format!("{d}{content}{d:#}\n"));
        } else {
            out.push_str(content);
            out.push('\n');
        }
    }
    Some(out)
}

fn parse_read_line(line: &str) -> Option<(usize, &str)> {
    let (prefix, rest) = line.split_once('\t')?;
    let num = prefix.trim().parse::<usize>().ok()?;
    Some((num, rest))
}

pub(crate) fn format_write_preview(args: &serde_json::Value, theme: &Theme, expanded: bool) -> Option<String> {
    let content = args.get("content")?.as_str()?;
    if content.trim().is_empty() {
        return None;
    }
    let lang = super::preview::detect_language_from_args(args);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let max = if expanded { total } else { 8.min(total) };
    let gutter_width = max.to_string().len().max(3);
    let mut out = format_preview_lines(&lines[..max], lang, gutter_width, theme);
    if !expanded && total > 8 {
        let d = theme.dimmed;
        out.push_str(&format!("{d}... ({} more lines, {total} total){d:#}\n", total - 8));
    }
    Some(out)
}

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

pub(crate) fn format_thinking_block(thinking_text: &str, theme: &Theme, width: usize) -> String {
    let d = theme.dimmed;
    let mut out = String::from("\n");
    let wrap_width = if width > 0 {
        width.saturating_sub(1).max(10)
    } else {
        crossterm::terminal::size()
            .map(|(w, _)| (w as usize).saturating_sub(1).max(10))
            .unwrap_or(79)
    };
    for line in thinking_text.trim().lines() {
        if line.trim().is_empty() {
            out.push_str(&format!("{d} {line}{d:#}\n"));
            continue;
        }
        for wrapped in crate::ui::interactive::wrap_to_width(line, wrap_width) {
            out.push_str(&format!("{d} {wrapped}{d:#}\n"));
        }
    }
    out
}
