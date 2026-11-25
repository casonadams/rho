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

pub fn append_section(content: &mut String, label: &str, body: &str, dim: anstyle::Style) {
    if !body.is_empty() {
        content.push_str(&format!("\n\n{dim}{label}{dim:#}\n{body}"));
    }
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
    let (icon, icon_style) = if tool.is_error {
        ("\u{2717}", theme.tool_err)
    } else {
        ("\u{2713}", theme.tool_ok)
    };
    let left = format_structured_header_left(&tool.name, icon, icon_style, tool.is_error, meta.as_deref(), theme);
    let duration = tool.duration_ms.map(super::format_duration_ms);
    let mut content = align_header_with_right(&left, duration.as_deref(), inner_width, theme.dimmed);
    append_section(&mut content, "Input", &input_body, theme.dimmed);
    if let Some(output_body) = structured_output_body(tool, width, expanded, show_read_expanded, theme) {
        append_section(&mut content, "Output", &output_body, theme.dimmed);
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
}
