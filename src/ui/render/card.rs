use super::diff::replace_tabs;
use super::file_change;
use super::formatters::{EditDiffRequest, format_edit_diff, format_read_expanded, format_write_preview};
use crate::ui::interactive::truncate_to_visual_lines;
use crate::ui::theme::{BlockStyle, Theme};
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_bash_full_summary, format_tool_args_summary, read_summary_parts,
};
use rho_harness_core::presentation::{SUBAGENT_STARTED_MARKER, ToolLine};

pub fn normalize_tool_name(name: &str) -> &str {
    match name {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    }
}

fn kind_from_format(format: &str) -> &'static str {
    match format.to_ascii_lowercase().as_str() {
        "pdf" => "pdf",
        "json" => "json",
        "csv" => "csv",
        "xml" => "xml",
        _ => "text",
    }
}

fn kind_from_url(url: &str) -> &'static str {
    if url.ends_with(".pdf") {
        "pdf"
    } else if url.ends_with(".json") {
        "json"
    } else if url.ends_with(".csv") {
        "csv"
    } else if url.ends_with(".xml") || url.ends_with(".rss") || url.ends_with(".atom") {
        "xml"
    } else {
        "text"
    }
}

pub fn fetch_content_kind(arguments: &serde_json::Value) -> &'static str {
    if let Some(format) = arguments.get("format").and_then(serde_json::Value::as_str) {
        return kind_from_format(format);
    }
    let url = arguments
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    kind_from_url(&url)
}

pub fn detect_language_from_args(args: &serde_json::Value) -> Option<&str> {
    let path = rho_harness_core::args::target_path(args)?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}

pub(crate) fn split_bash_timeout(summary: &str) -> (&str, Option<&str>) {
    if let Some(idx) = summary.rfind(" (timeout ")
        && summary.ends_with(')')
    {
        let timeout_part = &summary[idx + 1..];
        let inner = &timeout_part["(timeout ".len()..timeout_part.len() - 1];
        if inner.ends_with('s') && inner[..inner.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            return (&summary[..idx], Some(timeout_part));
        }
    }
    (summary, None)
}

pub fn format_bash_args_header(summary: &str, accent: anstyle::Style, dim: anstyle::Style) -> String {
    match split_bash_timeout(summary) {
        (cmd, Some(timeout_part)) => format!("{accent}{cmd}{accent:#} {dim}{timeout_part}{dim:#}"),
        (cmd, None) => format!("{accent}{cmd}{accent:#}"),
    }
}

/// Syntax-highlights a `bash` tool header using `syntect` ANSI-16 colors while keeping any trailing
/// `(timeout Ns)` suffix dim.
pub fn highlight_bash_args_header(summary: &str, theme: &Theme) -> String {
    let dim = theme.dimmed;
    match split_bash_timeout(summary) {
        (cmd, Some(timeout_part)) => {
            let highlighted = crate::ui::markdown::highlight_code_line(cmd, Some("sh"), theme);
            format!("{highlighted} {dim}{timeout_part}{dim:#}")
        }
        (cmd, None) => crate::ui::markdown::highlight_code_line(cmd, Some("sh"), theme),
    }
}

pub fn format_read_header(tool: &ToolLine, theme: &Theme) -> String {
    let (path, range) = read_summary_parts(&tool.arguments);
    let title = theme.tool_title_style(false);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |range| {
        let rs = theme.warning;
        format!("{rs}{range}{rs:#}")
    });
    match classify_read_path(&tool.arguments) {
        Some(ReadClassification::Skill { name }) => {
            let tag = theme.skill_tag;
            let offset = tool
                .arguments
                .get("offset")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1);
            let suffix = if offset <= 1 { "" } else { &range_suffix };
            format!("{tag}[skill]{tag:#} {name}{suffix}")
        }
        Some(ReadClassification::Resource { path }) => {
            format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        Some(ReadClassification::Docs { path }) => {
            format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        None => format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}"),
    }
}

pub fn format_tool_header(tool: &ToolLine, theme: &Theme) -> String {
    format_tool_header_with_expansion(tool, theme, false)
}

pub fn format_tool_header_with_expansion(tool: &ToolLine, theme: &Theme, expanded: bool) -> String {
    let mut header = format_tool_header_base(tool, theme, expanded);
    if let Some(details) = file_change::header_details(tool) {
        let dim = theme.dimmed;
        header.push_str(&format!("{dim} \u{b7} {details}{dim:#}"));
    }
    header
}

pub(crate) fn style_tool_header_args(tool_name: &str, summary: &str, theme: &Theme) -> String {
    if tool_name == "bash" {
        return if theme.syntax_highlight_tools {
            highlight_bash_args_header(summary, theme)
        } else {
            format_bash_args_header(summary, theme.highlight, theme.dimmed)
        };
    }
    if theme.syntax_highlight_tools && (summary.starts_with('{') || summary.starts_with('[')) {
        return crate::ui::markdown::highlight_code_line(summary, Some("json"), theme);
    }
    let accent = theme.highlight;
    format!("{accent}{summary}{accent:#}")
}

fn format_tool_header_base(tool: &ToolLine, theme: &Theme, expanded: bool) -> String {
    let title = theme.tool_title_style(tool.is_error);
    let accent = theme.highlight;
    let display_name = normalize_tool_name(&tool.name);
    if !tool.is_error && tool.name == "read" {
        return format_read_header(tool, theme);
    }
    if !tool.is_error && display_name == "web_fetch" {
        let url = tool
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let dim = theme.dimmed;
        let kind = fetch_content_kind(&tool.arguments);
        return format!("{title}web_fetch{title:#} {accent}{url}{accent:#} {dim}({kind}){dim:#}");
    }
    let summary = if tool.name == "bash" && (theme.full_bash_command || expanded) {
        format_bash_full_summary(&tool.arguments)
    } else {
        format_tool_args_summary(&tool.name, &tool.arguments)
    };
    if summary.is_empty() {
        format!("{title}{display_name}{title:#}")
    } else {
        let header_args = style_tool_header_args(&tool.name, &summary, theme);
        format!("{title}{display_name}{title:#} {header_args}")
    }
}

pub(crate) fn raw_output(tool: &ToolLine) -> &str {
    if tool.output.is_empty() {
        &tool.output_summary
    } else {
        &tool.output
    }
}

/// Adds `section` to the card body behind exactly one blank row, whatever newlines either side ends with.
pub(crate) fn push_section(content: &mut String, section: &str) {
    content.truncate(content.trim_end_matches('\n').len());
    content.push_str("\n\n");
    content.push_str(section.trim_end_matches('\n'));
}

/// Draws finished card content inside the tool block frame.
pub(crate) fn frame_tool_card(content: &str, is_bash: bool, is_error: bool, width: usize, theme: &Theme) -> String {
    theme
        .tool_block(is_bash, is_error, width)
        .with_vertical_padding()
        .render_styled(content)
}

fn append_read_expanded(content: &mut String, tool: &ToolLine, theme: &Theme) {
    if let Some(formatted) = format_read_expanded(raw_output(tool), &tool.arguments, theme) {
        push_section(content, &formatted);
    }
}

pub(crate) fn format_output_lines(clean: &str, width: usize, expanded: bool, dim: anstyle::Style) -> String {
    if expanded {
        clean.to_string()
    } else {
        let truncated = truncate_to_visual_lines(clean, 5, width.saturating_sub(4).max(1));
        let mut out = String::new();
        if truncated.skipped_count > 0 {
            out.push_str(&format!(
                "{dim}... ({n} earlier lines, Ctrl+O to expand){dim:#}\n",
                n = truncated.skipped_count
            ));
        }
        out.push_str(&truncated.visual_lines.join("\n"));
        out
    }
}

fn append_output_lines(content: &mut String, clean: &str, width: usize, expanded: bool, dim: anstyle::Style) {
    push_section(content, &format_output_lines(clean, width, expanded, dim));
}

fn append_generic_output(content: &mut String, tool: &ToolLine, width: usize, expanded: bool, theme: &Theme) {
    let clean = replace_tabs(raw_output(tool).trim_end());
    if !clean.is_empty() {
        let rendered = if theme.syntax_highlight_tools {
            super::output_highlight::highlight_tool_output(&tool.name, &clean, theme)
        } else {
            clean
        };
        append_output_lines(content, &rendered, width, expanded, theme.dimmed);
    }
}

pub(crate) fn edit_diff(tool: &ToolLine, expanded: bool, theme: &Theme) -> Option<String> {
    let request = EditDiffRequest {
        args: &tool.arguments,
        file_change: tool.file_change.as_ref(),
        expanded,
    };
    format_edit_diff(request, theme)
}

pub(crate) fn append_failed_edit_attempt(content: &mut String, tool: &ToolLine, expanded: bool, theme: &Theme) {
    if let Some(diff) = edit_diff(tool, expanded, theme) {
        let dim = theme.dimmed;
        push_section(content, &format!("{dim}Attempted change:{dim:#}\n{diff}"));
    }
}

fn append_edit_or_write(content: &mut String, tool: &ToolLine, width: usize, expanded: bool, theme: &Theme) -> bool {
    if !tool.is_error && tool.name == "edit" {
        if let Some(diff) = edit_diff(tool, expanded, theme) {
            push_section(content, &diff);
        }
        return true;
    }
    if !tool.is_error && tool.name == "write" {
        let inner_width = theme.tool_block(false, false, width).inner_width();
        if let Some(preview) = format_write_preview(&tool.arguments, theme, expanded, inner_width) {
            push_section(content, &preview);
        }
        return true;
    }
    false
}

pub(crate) fn append_background_hint(content: &mut String, tool: &ToolLine, dim: anstyle::Style) {
    if tool.name == "subagent" && !tool.is_error && raw_output(tool).starts_with(SUBAGENT_STARTED_MARKER) {
        push_section(
            content,
            &format!("{dim}running in the background - press \u{2190} on an empty prompt to watch{dim:#}"),
        );
    }
}

pub(crate) fn append_tool_details(content: &mut String, tool: &ToolLine, width: usize, view: CardView, theme: &Theme) {
    if !tool.is_error && tool.name == "read" {
        if view.show_read_expanded {
            append_read_expanded(content, tool, theme);
        }
    } else if !append_edit_or_write(content, tool, width, view.expanded, theme) && shows_output_body(tool, view) {
        append_generic_output(content, tool, width, view.expanded, theme);
    }
    if tool.is_error && tool.name == "edit" {
        append_failed_edit_attempt(content, tool, view.expanded, theme);
    }
    append_background_hint(content, tool, theme.dimmed);
    if let Some(duration_ms) = tool.duration_ms {
        let dim = theme.dimmed;
        push_section(
            content,
            &format!("{dim}Took {}{dim:#}", super::format_duration_ms(duration_ms)),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardStatus {
    Running,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardView {
    pub status: CardStatus,
    pub expanded: bool,
    pub show_read_expanded: bool,
}

pub(crate) fn shows_output_body(tool: &ToolLine, view: CardView) -> bool {
    tool.name == "bash" || tool.is_error || view.expanded || view.status == CardStatus::Running
}

pub fn render_tool_block(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let view = CardView {
        status: CardStatus::Finished,
        expanded,
        show_read_expanded,
    };
    render_card(tool, view, width, theme)
}

/// The live card of a running tool: the committed card with `duration_ms` counting up.
pub(crate) fn render_running_tool_block(tool: &ToolLine, width: usize, expanded: bool, theme: &Theme) -> String {
    let view = CardView {
        status: CardStatus::Running,
        expanded,
        show_read_expanded: expanded,
    };
    render_card(tool, view, width, theme)
}

fn render_card(tool: &ToolLine, view: CardView, width: usize, theme: &Theme) -> String {
    let content = if theme.structured_tool_cards {
        super::structured::render_structured_tool_content(tool, view, width, theme)
    } else {
        let mut content = format_tool_header_with_expansion(tool, theme, view.expanded);
        append_tool_details(&mut content, tool, width, view, theme);
        content
    };
    frame_tool_card(&content, tool.name == "bash", tool.is_error, width, theme)
}

pub fn render_tool_transcript(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let block = render_tool_block(tool, width, expanded, show_read_expanded, theme);
    if theme.block_style == BlockStyle::Border {
        block
    } else {
        format!("\n{block}")
    }
}
