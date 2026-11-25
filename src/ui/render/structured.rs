//! Structured tool card formatting (`ui.structured_tool_cards = true`).
//!
//! Renders a clean status + Title Case header with right-aligned duration,
//! followed by explicit `Input` and `Output` sections.

use super::card::{
    append_background_hint, append_failed_edit_attempt, edit_diff, fetch_content_kind, format_output_lines,
    normalize_tool_name, raw_output, split_bash_timeout, style_tool_header_args,
};
use super::diff::replace_tabs;
use super::file_change;
use super::formatters::{format_read_expanded, format_write_preview};
use crate::ui::block::visible_width;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_bash_full_summary, format_tool_args_summary, read_summary_parts,
};

fn title_case_words(name: &str) -> String {
    name.split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn format_tool_title(name: &str) -> String {
    match normalize_tool_name(name) {
        "bash" => "Bash".to_string(),
        "read" => "Read".to_string(),
        "edit" => "Edit".to_string(),
        "write" => "Write".to_string(),
        "rg" => "Rg".to_string(),
        "fd" => "Fd".to_string(),
        "lsp" => "LSP".to_string(),
        "mcp" => "MCP".to_string(),
        "web_search" => "Web Search".to_string(),
        "web_fetch" => "Web Fetch".to_string(),
        "subagent" => "Subagent".to_string(),
        "todo" => "Todo".to_string(),
        "undo" => "Undo".to_string(),
        "wait" => "Wait".to_string(),
        "worktree" => "Worktree".to_string(),
        "memory" => "Memory".to_string(),
        "tool_search" => "Tool Search".to_string(),
        other => title_case_words(other),
    }
}

pub fn align_header_with_right(left: &str, right: Option<&str>, inner_width: usize, dim: anstyle::Style) -> String {
    let Some(right_text) = right.filter(|s| !s.is_empty()) else {
        return left.to_string();
    };
    let right_styled = format!("{dim}{right_text}{dim:#}");
    let left_w = visible_width(left);
    let right_w = visible_width(right_text);
    let gap = inner_width
        .checked_sub(left_w.saturating_add(right_w))
        .filter(|&g| g >= 2)
        .unwrap_or(2);
    format!("{left}{}{right_styled}", " ".repeat(gap))
}

pub fn format_structured_header_left(
    name: &str,
    status_icon: &str,
    status_style: anstyle::Style,
    is_error: bool,
    meta: Option<&str>,
    theme: &Theme,
) -> String {
    let title_style = theme.tool_title_style(is_error);
    let title = format_tool_title(name);
    let mut header = format!("{status_style}{status_icon}{status_style:#} {title_style}{title}{title_style:#}");
    if let Some(details) = meta.filter(|s| !s.is_empty()) {
        let dim = theme.dimmed;
        header.push_str(&format!("{dim} \u{b7} {details}{dim:#}"));
    }
    header
}

pub fn format_structured_bash_input(summary: &str, theme: &Theme) -> (String, Option<String>) {
    let dim = theme.dimmed;
    let accent = theme.highlight;
    let (cmd, timeout_part) = split_bash_timeout(summary);
    let meta = timeout_part.map(|t| t.trim_start_matches('(').trim_end_matches(')').to_string());
    if cmd.is_empty() {
        return (String::new(), meta);
    }
    let styled_cmd = if theme.syntax_highlight_tools {
        crate::ui::markdown::highlight_code_line(cmd, Some("sh"), theme)
    } else {
        format!("{accent}{cmd}{accent:#}")
    };
    (format!("{dim}${dim:#} {styled_cmd}"), meta)
}

pub fn append_section(content: &mut String, label: &str, body: &str, inner_width: usize, dim: anstyle::Style) {
    if body.is_empty() {
        return;
    }
    let label_width = visible_width(label);
    if inner_width < label_width.saturating_add(4) {
        content.push_str(&format!("\n{dim}{label}{dim:#}\n{body}"));
        return;
    }
    let dashes = "─".repeat(inner_width - label_width - 3);
    content.push_str(&format!("\n{dim}─ {label} {dashes}{dim:#}\n{body}"));
}

const SUBAGENT_PREVIEW_MAX_LINES: usize = 4;
const SUBAGENT_PREVIEW_MAX_CHARS: usize = 320;

fn str_field<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn truncate_chars(text: &str, max_chars: usize) -> (&str, bool) {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => (&text[..idx], true),
        None => (text, false),
    }
}

fn format_subagent_prompt_preview(prompt: &str, expanded: bool, dim: anstyle::Style) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() || expanded {
        return trimmed.to_string();
    }
    let all_lines: Vec<&str> = trimmed.lines().collect();
    let mut kept = Vec::new();
    let mut used_chars = 0usize;
    let mut truncated_by_chars = false;

    for line in all_lines.iter().take(SUBAGENT_PREVIEW_MAX_LINES) {
        let remaining_chars = SUBAGENT_PREVIEW_MAX_CHARS.saturating_sub(used_chars);
        if remaining_chars == 0 {
            truncated_by_chars = true;
            break;
        }
        let (slice, was_cut) = truncate_chars(line, remaining_chars);
        used_chars += slice.chars().count();
        if was_cut {
            kept.push(format!("{slice}\u{2026}"));
            truncated_by_chars = true;
            break;
        }
        kept.push(slice.to_string());
    }

    let remaining_lines = all_lines.len().saturating_sub(kept.len());
    if remaining_lines > 0 {
        kept.push(format!("{dim}\u{2026} (+{remaining_lines} more lines){dim:#}"));
    } else if truncated_by_chars && !kept.last().is_some_and(|l| l.ends_with('\u{2026}')) {
        kept.push(format!("{dim}\u{2026}{dim:#}"));
    }
    kept.join("\n")
}

pub(crate) fn structured_subagent_input_and_meta(
    tool: &ToolLine,
    expanded: bool,
    theme: &Theme,
) -> (String, Option<String>) {
    let args = &tool.arguments;
    let name = str_field(args, "name");
    let task_id = str_field(args, "task_id");
    let inspect = args
        .get("inspect")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let role = str_field(args, "agent")
        .or_else(|| str_field(args, "role"))
        .or_else(|| (!inspect).then_some("scout"));
    let mut parts = Vec::new();
    if let Some(id) = name.or(task_id) {
        parts.push(id.to_string());
    }
    if let Some(r) = role {
        parts.push(r.to_string());
    }
    if let Some(model) = str_field(args, "model") {
        parts.push(format!("model: {model}"));
    }
    if let Some(persona) = str_field(args, "persona") {
        parts.push(format!("persona: {persona}"));
    }
    if inspect {
        parts.push("inspect".to_string());
    }
    let meta = (!parts.is_empty()).then(|| parts.join(" \u{b7} "));
    let prompt = str_field(args, "prompt").unwrap_or("");
    let input = if prompt.is_empty() && inspect {
        match task_id {
            Some(id) => format!("inspect task {id}"),
            None => "list subagent tasks".to_string(),
        }
    } else {
        format_subagent_prompt_preview(prompt, expanded, theme.dimmed)
    };
    (input, meta)
}

fn structured_read_input_and_meta(tool: &ToolLine, theme: &Theme) -> (String, Option<String>) {
    let (path, range) = read_summary_parts(&tool.arguments);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |r| {
        let warn = theme.warning;
        format!("{warn}{r}{warn:#}")
    });
    let input = if path.is_empty() {
        String::new()
    } else {
        format!("{accent}{path}{accent:#}{range_suffix}")
    };
    let meta = match classify_read_path(&tool.arguments) {
        Some(ReadClassification::Skill { name }) => Some(format!("skill \u{b7} {name}")),
        Some(ReadClassification::Resource { .. }) => Some("resource".to_string()),
        Some(ReadClassification::Docs { .. }) => Some("docs".to_string()),
        None => None,
    };
    (input, meta)
}

fn structured_tool_input_and_meta(tool: &ToolLine, expanded: bool, theme: &Theme) -> (String, Option<String>) {
    let display_name = normalize_tool_name(&tool.name);
    if tool.name == "bash" {
        let summary = if theme.full_bash_command || expanded {
            format_bash_full_summary(&tool.arguments)
        } else {
            format_tool_args_summary(&tool.name, &tool.arguments)
        };
        return format_structured_bash_input(&summary, theme);
    }
    if display_name == "subagent" {
        return structured_subagent_input_and_meta(tool, expanded, theme);
    }
    if !tool.is_error && tool.name == "read" {
        return structured_read_input_and_meta(tool, theme);
    }
    if !tool.is_error && display_name == "web_fetch" {
        let url = tool
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let accent = theme.highlight;
        let input = if url.is_empty() {
            String::new()
        } else {
            format!("{accent}{url}{accent:#}")
        };
        return (input, Some(fetch_content_kind(&tool.arguments).to_string()));
    }
    let summary = format_tool_args_summary(&tool.name, &tool.arguments);
    let input = if summary.is_empty() {
        String::new()
    } else {
        style_tool_header_args(&tool.name, &summary, theme)
    };
    (input, file_change::header_details(tool))
}

fn structured_generic_output(tool: &ToolLine, width: usize, expanded: bool, theme: &Theme) -> Option<String> {
    let clean = replace_tabs(raw_output(tool).trim_end());
    if clean.is_empty() {
        return None;
    }
    let rendered = if theme.syntax_highlight_tools {
        super::output_highlight::highlight_tool_output(&tool.name, &clean, theme)
    } else {
        clean
    };
    Some(format_output_lines(&rendered, width, expanded, theme.dimmed))
}

fn structured_output_body(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> Option<String> {
    if !tool.is_error && tool.name == "read" {
        return if show_read_expanded {
            format_read_expanded(raw_output(tool), &tool.arguments, theme)
        } else {
            None
        };
    }
    if !tool.is_error && tool.name == "edit" {
        return edit_diff(tool, expanded, theme);
    }
    if !tool.is_error && tool.name == "write" {
        return format_write_preview(&tool.arguments, theme, expanded);
    }
    if tool.name == "bash" || tool.is_error || expanded {
        return structured_generic_output(tool, width, expanded, theme);
    }
    None
}

fn append_running_meta(meta: Option<String>) -> Option<String> {
    match meta {
        Some(m) if !m.contains("running") => Some(format!("{m} \u{b7} running")),
        None => Some("running".to_string()),
        other => other,
    }
}

pub fn render_structured_tool_content(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let inner_width = theme
        .tool_block(tool.name == "bash", tool.is_error, width)
        .inner_width();
    let (input_body, meta) = structured_tool_input_and_meta(tool, expanded, theme);
    let is_subagent_running = !tool.is_error
        && tool.name == "subagent"
        && raw_output(tool).trim_start().starts_with("Started subagent task ");
    let (icon, icon_style) = if tool.is_error {
        ("\u{2717}", theme.tool_err)
    } else if is_subagent_running {
        ("\u{25cf}", theme.highlight)
    } else {
        ("\u{2713}", theme.tool_ok)
    };
    let meta = if is_subagent_running {
        append_running_meta(meta)
    } else {
        meta
    };
    let left = format_structured_header_left(&tool.name, icon, icon_style, tool.is_error, meta.as_deref(), theme);
    let duration = tool.duration_ms.map(super::format_duration_ms);
    let mut content = align_header_with_right(&left, duration.as_deref(), inner_width, theme.dimmed);
    append_section(&mut content, "Input", &input_body, inner_width, theme.dimmed);
    if let Some(output_body) = structured_output_body(tool, inner_width, expanded, show_read_expanded, theme) {
        append_section(&mut content, "Output", &output_body, inner_width, theme.dimmed);
    }
    if tool.is_error && tool.name == "edit" {
        append_failed_edit_attempt(&mut content, tool, expanded, theme);
    }
    append_background_hint(&mut content, tool, theme.dimmed);
    content
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::block::strip_ansi;
    use crate::ui::render::card::render_tool_block;
    use serde_json::json;

    fn structured_theme() -> Theme {
        Theme {
            structured_tool_cards: true,
            ..Theme::default()
        }
    }

    #[test]
    fn structured_bash_card_renders_header_duration_input_and_output() {
        let tool = ToolLine {
            name: "bash".to_string(),
            arguments: json!({"command": "echo hi", "timeout": 30}),
            output: "hi\n".to_string(),
            output_summary: "hi".to_string(),
            is_error: false,
            duration_ms: Some(3400),
            file_change: None,
        };
        let rendered = strip_ansi(&render_tool_block(&tool, 60, false, false, &structured_theme()));
        assert!(rendered.contains("✓ Bash · timeout 30s"), "got:\n{rendered}");
        assert!(rendered.contains("3.400s"), "got:\n{rendered}");
        assert!(rendered.contains("Input"), "got:\n{rendered}");
        assert!(rendered.contains("$ echo hi"), "got:\n{rendered}");
        assert!(rendered.contains("Output"), "got:\n{rendered}");
        assert!(!rendered.contains("Took "), "duration should be in header:\n{rendered}");
    }

    #[test]
    fn structured_error_card_uses_cross_icon() {
        let tool = ToolLine {
            name: "bash".to_string(),
            arguments: json!({"command": "false"}),
            output: "exit 1".to_string(),
            output_summary: "exit 1".to_string(),
            is_error: true,
            duration_ms: Some(12),
            file_change: None,
        };
        let rendered = strip_ansi(&render_tool_block(&tool, 60, false, false, &structured_theme()));
        assert!(rendered.contains("✗ Bash"), "got:\n{rendered}");
        assert!(rendered.contains("12ms"), "got:\n{rendered}");
    }

    #[test]
    fn structured_read_card_places_path_under_input_and_omits_empty_output() {
        let tool = ToolLine {
            name: "read".to_string(),
            arguments: json!({"path": "src/main.rs", "offset": 1, "limit": 20}),
            output: "fn main() {}".to_string(),
            output_summary: "1 line".to_string(),
            is_error: false,
            duration_ms: Some(5),
            file_change: None,
        };
        let rendered = strip_ansi(&render_tool_block(&tool, 60, false, false, &structured_theme()));
        assert!(rendered.contains("✓ Read"), "got:\n{rendered}");
        assert!(rendered.contains("5ms"), "got:\n{rendered}");
        assert!(rendered.contains("Input"), "got:\n{rendered}");
        assert!(rendered.contains("src/main.rs:1-20"), "got:\n{rendered}");
        assert!(
            !rendered.contains("Output"),
            "collapsed read has no output:\n{rendered}"
        );
    }

    #[test]
    fn default_theme_keeps_legacy_tool_card_layout() {
        let tool = ToolLine {
            name: "bash".to_string(),
            arguments: json!({"command": "echo hi"}),
            output: "hi".to_string(),
            output_summary: "hi".to_string(),
            is_error: false,
            duration_ms: Some(50),
            file_change: None,
        };
        let rendered = strip_ansi(&render_tool_block(&tool, 60, false, false, &Theme::default()));
        assert!(rendered.contains("bash echo hi"), "got:\n{rendered}");
        assert!(rendered.contains("Took 50ms"), "got:\n{rendered}");
        assert!(!rendered.contains("✓ Bash"), "got:\n{rendered}");
    }

    #[test]
    fn structured_sections_render_horizontal_dividers_at_inner_width() {
        use crate::ui::theme::BlockStyle;
        use anstyle::{AnsiColor, Color, Style};

        let tool = ToolLine {
            name: "bash".to_string(),
            arguments: json!({"command": "sleep 10"}),
            output: "[exit 0; no output]".to_string(),
            output_summary: "[exit 0; no output]".to_string(),
            is_error: false,
            duration_ms: Some(10_000),
            file_change: None,
        };
        let width = 60;
        let bg = Style::new().bg_color(Some(Color::Ansi(AnsiColor::Black)));
        let theme = Theme {
            structured_tool_cards: true,
            block_style: BlockStyle::Divider,
            block_fill: bg,
            ..Theme::default()
        };
        let inner_width = theme.tool_block(true, false, width).inner_width();
        let raw_rendered = render_tool_block(&tool, width, false, false, &theme);
        let stripped = strip_ansi(&raw_rendered);
        let lines: Vec<&str> = stripped.lines().collect();

        assert!(
            lines.iter().all(|line| visible_width(line) == width),
            "every line must match outer width {width}:\n{stripped}"
        );
        assert!(
            !stripped.contains(['╭', '╮', '╰', '╯', '│']),
            "must not contain nested box borders:\n{stripped}"
        );

        let input_divider = format!("─ Input {}", "─".repeat(inner_width - visible_width("─ Input ")));
        let output_divider = format!("─ Output {}", "─".repeat(inner_width - visible_width("─ Output ")));
        assert_eq!(lines[2].trim(), input_divider, "got:\n{stripped}");
        assert_eq!(lines[3].trim(), "$ sleep 10", "got:\n{stripped}");
        assert_eq!(lines[4].trim(), output_divider, "got:\n{stripped}");
        assert_eq!(lines[5].trim(), "[exit 0; no output]", "got:\n{stripped}");

        for raw_line in raw_rendered.lines().filter(|l| l.contains("─ Input ")) {
            assert!(
                raw_line.starts_with("\x1b[40m "),
                "expected block_fill background on divider line: {raw_line:?}"
            );
            assert!(
                raw_line.contains("\x1b[0m\x1b[40m"),
                "expected block_fill re-applied after inner reset: {raw_line:?}"
            );
        }
    }

    #[test]
    fn append_section_aligns_divider_to_exact_width_and_falls_back_when_narrow() {
        let mut content = String::new();
        append_section(&mut content, "Input", "x", 20, anstyle::Style::new());
        let stripped = strip_ansi(&content);
        let divider = stripped.lines().nth(1).expect("divider line");
        assert_eq!(visible_width(divider), 20);
        assert_eq!(divider, "─ Input ────────────");

        let mut min_fit = String::new();
        append_section(&mut min_fit, "Input", "x", 9, anstyle::Style::new());
        let min_stripped = strip_ansi(&min_fit);
        let min_divider = min_stripped.lines().nth(1).expect("min divider line");
        assert_eq!(visible_width(min_divider), 9);
        assert_eq!(min_divider, "─ Input ─");

        let mut narrow = String::new();
        append_section(&mut narrow, "Input", "x", 8, anstyle::Style::new());
        let narrow_stripped = strip_ansi(&narrow);
        assert_eq!(narrow_stripped, "\nInput\nx");
        assert!(!narrow_stripped.contains('─'));
    }

    #[test]
    fn subagent_card_shows_metadata_and_multiline_prompt_preview() {
        let tool = ToolLine {
            name: "subagent".to_string(),
            arguments: json!({
                "name": "localWordCompletionAgent",
                "role": "general",
                "model": "smol",
                "prompt": "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6"
            }),
            output: "Done".to_string(),
            output_summary: "Done".to_string(),
            is_error: false,
            duration_ms: Some(1200),
            file_change: None,
        };
        let rendered = strip_ansi(&render_tool_block(&tool, 80, false, false, &structured_theme()));
        assert!(
            rendered.contains("✓ Subagent · localWordCompletionAgent · general · model: smol"),
            "got:\n{rendered}"
        );
        assert!(rendered.contains("Line 1"), "got:\n{rendered}");
        assert!(rendered.contains("Line 4"), "got:\n{rendered}");
        assert!(
            !rendered.contains("Line 5"),
            "collapsed should cap at 4 lines:\n{rendered}"
        );
        assert!(rendered.contains("… (+2 more lines)"), "got:\n{rendered}");

        let expanded = strip_ansi(&render_tool_block(&tool, 80, true, false, &structured_theme()));
        assert!(
            expanded.contains("Line 6"),
            "expanded should show full prompt:\n{expanded}"
        );
    }

    #[test]
    fn subagent_async_launch_uses_running_icon_and_running_meta() {
        let tool = ToolLine {
            name: "subagent".to_string(),
            arguments: json!({
                "name": "auditAgent",
                "prompt": "Inspect all crates for unsafe blocks."
            }),
            output: "Started subagent task 'auditAgent' (scout). Its result arrives as a follow-up message."
                .to_string(),
            output_summary: "Started subagent task 'auditAgent'".to_string(),
            is_error: false,
            duration_ms: Some(15),
            file_change: None,
        };
        let raw = render_tool_block(&tool, 80, false, false, &structured_theme());
        let stripped = strip_ansi(&raw);
        assert!(
            stripped.contains("● Subagent · auditAgent · scout · running"),
            "got:\n{stripped}"
        );
        assert!(
            !stripped.contains("✓ Subagent"),
            "should not show green checkmark:\n{stripped}"
        );
        assert!(
            stripped.contains("Inspect all crates for unsafe blocks."),
            "got:\n{stripped}"
        );
    }

    #[test]
    fn subagent_inspect_without_prompt_shows_clear_input_description() {
        let (input, meta) = structured_subagent_input_and_meta(
            &ToolLine {
                name: "subagent".to_string(),
                arguments: json!({"task_id": "t-42", "inspect": true}),
                output: String::new(),
                output_summary: String::new(),
                is_error: false,
                duration_ms: None,
                file_change: None,
            },
            false,
            &structured_theme(),
        );
        assert_eq!(input, "inspect task t-42");
        assert_eq!(meta.as_deref(), Some("t-42 · inspect"));
    }
}
