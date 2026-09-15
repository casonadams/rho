//! Edit-diff, write-preview, thinking-block, session-status, and relative-time formatters.
//!
//! These are `pub(crate)` because they are only consumed by `renderer.rs`,
//! but they remain exposed as module-private items so future tools can reuse them.

use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use rho_harness_core::presentation::SessionStatus;

pub fn find_edit_line_number(path_str: &str, old_text: &str, new_text: &str) -> Option<usize> {
    let content = std::fs::read_to_string(path_str).ok()?;
    locate_match_line(&content, old_text).or_else(|| locate_match_line(&content, new_text))
}

fn locate_match_line(content: &str, target: &str) -> Option<usize> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(idx) = content.find(target) {
        return Some(1 + content[..idx].matches('\n').count());
    }
    let norm_content = content.replace("\r\n", "\n");
    let norm_target = target.replace("\r\n", "\n");
    if let Some(idx) = norm_content.find(&norm_target) {
        return Some(1 + norm_content[..idx].matches('\n').count());
    }
    let first_line = target.lines().find(|l| !l.trim().is_empty())?;
    if content.matches(first_line).count() == 1 {
        let idx = content.find(first_line)?;
        return Some(1 + content[..idx].matches('\n').count());
    }
    None
}

pub fn format_gutter_prefix(line_num: usize, gutter_width: usize, dim: anstyle::Style) -> String {
    format!("{dim}{line_num:>gutter_width$} │ {dim:#}")
}

pub fn detect_language_from_args(args: &serde_json::Value) -> Option<&str> {
    let path = args.get("path").or_else(|| args.get("file_path"))?.as_str()?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}

fn split_leading_whitespace(token: &str) -> (&str, &str) {
    let non_ws_idx = token.find(|c: char| !c.is_whitespace()).unwrap_or(token.len());
    (&token[..non_ws_idx], &token[non_ws_idx..])
}

fn push_inverted_token(buf: &mut String, text: &str) {
    let (ws, non_ws) = split_leading_whitespace(text);
    buf.push_str(ws);
    if !non_ws.is_empty() {
        buf.push_str("\x1b[7m");
        buf.push_str(non_ws);
        buf.push_str("\x1b[27m");
    }
}

pub fn render_single_line_word_diff(old_line: &str, new_line: &str, theme: &Theme) -> (String, String) {
    let clean_old = old_line.replace('\t', "   ");
    let clean_new = new_line.replace('\t', "   ");
    let (red, green) = (theme.tool_err, theme.tool_ok);
    let mut removed_buf = format!("{red}- ");
    let mut added_buf = format!("{green}+ ");

    let diff = similar::TextDiff::from_words(&clean_old, &clean_new);
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                removed_buf.push_str(change.value());
                added_buf.push_str(change.value());
            }
            similar::ChangeTag::Delete => {
                push_inverted_token(&mut removed_buf, change.value());
            }
            similar::ChangeTag::Insert => {
                push_inverted_token(&mut added_buf, change.value());
            }
        }
    }
    removed_buf.push_str("\x1b[0m\n");
    added_buf.push_str("\x1b[0m\n");
    (removed_buf, added_buf)
}

pub struct EntryDiffInput<'a> {
    pub idx: usize,
    pub old_text: &'a str,
    pub new_text: &'a str,
    pub theme: &'a Theme,
    pub start_line: Option<usize>,
}

fn push_edit_header(out: &mut String, idx: usize, start_line: Option<usize>, dim: anstyle::Style) {
    if idx == 0 {
        return;
    }
    if let Some(line) = start_line {
        out.push_str(&format!("{dim}@@ edit #{} · line {line} @@{dim:#}\n", idx + 1));
    } else {
        out.push_str(&format!("{dim}@@ edit #{} @@{dim:#}\n", idx + 1));
    }
}

fn push_diff_lines(
    out: &mut String,
    lines: &[&str],
    is_add: bool,
    start_line: Option<usize>,
    gutter_width: usize,
    theme: &Theme,
) {
    let (prefix, color) = if is_add {
        ('+', theme.tool_ok)
    } else {
        ('-', theme.tool_err)
    };
    let dim = theme.dimmed;
    for (offset, line) in lines.iter().take(8).enumerate() {
        let clean = line.replace('\t', "   ");
        if let Some(start) = start_line {
            let line_num = start + offset;
            let gutter = format_gutter_prefix(line_num, gutter_width, dim);
            out.push_str(&format!("{gutter}{color}{prefix} {clean}{color:#}\n"));
        } else {
            out.push_str(&format!("{color}{prefix} {clean}{color:#}\n"));
        }
    }
    if lines.len() > 8 {
        out.push_str(&format!("{dim}... ({} more lines){dim:#}\n", lines.len() - 8));
    }
}

pub fn format_entry_diff(input: EntryDiffInput<'_>) -> String {
    let mut out = String::new();
    push_edit_header(&mut out, input.idx, input.start_line, input.theme.dimmed);

    let old_lines: Vec<&str> = input.old_text.lines().collect();
    let new_lines: Vec<&str> = input.new_text.lines().collect();
    let max_line = input
        .start_line
        .map(|start| start + old_lines.len().max(new_lines.len()))
        .unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3);

    if old_lines.len() == 1 && new_lines.len() == 1 {
        let (removed, added) = render_single_line_word_diff(old_lines[0], new_lines[0], input.theme);
        if let Some(line) = input.start_line {
            let prefix = format_gutter_prefix(line, gutter_width, input.theme.dimmed);
            out.push_str(&format!("{prefix}{removed}"));
            out.push_str(&format!("{prefix}{added}"));
        } else {
            out.push_str(&removed);
            out.push_str(&added);
        }
    } else {
        push_diff_lines(&mut out, &old_lines, false, input.start_line, gutter_width, input.theme);
        push_diff_lines(&mut out, &new_lines, true, input.start_line, gutter_width, input.theme);
    }
    out
}

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
            .or_else(|| path_str.and_then(|p| find_edit_line_number(p, old_text, new_text)));

        out.push_str(&format_entry_diff(EntryDiffInput {
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
        let gutter = format_gutter_prefix(line_num, gutter_width, d);
        out.push_str(&format!("{gutter}{highlighted}\n"));
    }
    out
}

pub(crate) fn format_read_expanded(raw: &str, args: &serde_json::Value, theme: &Theme) -> Option<String> {
    let clean = raw.trim_end();
    if clean.is_empty() {
        return None;
    }
    let lang = detect_language_from_args(args);
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
            let gutter = format_gutter_prefix(num, gutter_width, d);
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
    let lang = detect_language_from_args(args);
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
        usize::from(crate::ui::terminal_width().saturating_sub(1).max(10))
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
