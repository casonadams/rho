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
        out.push_str(&format!("{d}{line_num:>gutter_width$} │ {d:#}{highlighted}\n"));
    }
    out
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

pub(crate) fn format_thinking_block(thinking_text: &str, theme: &Theme) -> String {
    let d = theme.dimmed;
    let mut out = String::from("\n");
    for line in thinking_text.trim().lines() {
        out.push_str(&format!("{d} {line}{d:#}\n"));
    }
    out
}
