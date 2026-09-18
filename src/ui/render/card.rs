use super::formatters::{format_edit_diff, format_read_expanded, format_write_preview};
use crate::ui::block::terminal_width;
use crate::ui::interactive::truncate_to_visual_lines;
use crate::ui::theme::{BlockStyle, Theme};
use rho_harness_core::presentation::ToolLine;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_tool_args_summary, read_summary_parts,
};

pub fn normalize_tool_name(name: &str) -> &str {
    match name {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    }
}

pub fn tool_title_style(is_error: bool) -> anstyle::Style {
    if is_error {
        anstyle::Style::new()
            .bold()
            .fg_color(Some(anstyle::AnsiColor::Red.into()))
    } else {
        anstyle::Style::new().bold()
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
    let path = args.get("path").or_else(|| args.get("file_path"))?.as_str()?;
    detect_language_from_path(path)
}

pub fn detect_language_from_path(path: &str) -> Option<&str> {
    std::path::Path::new(path).extension()?.to_str()
}

pub fn format_bash_args_header(summary: &str, accent: anstyle::Style, dim: anstyle::Style) -> String {
    if let Some(idx) = summary.rfind(" (timeout ")
        && summary.ends_with(')')
    {
        let timeout_part = &summary[idx + 1..];
        let inner = &timeout_part["(timeout ".len()..timeout_part.len() - 1];
        if inner.ends_with('s') && inner[..inner.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            let cmd = &summary[..idx];
            return format!("{accent}{cmd}{accent:#} {dim}{timeout_part}{dim:#}");
        }
    }
    format!("{accent}{summary}{accent:#}")
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
            format!("{tag}[skill]{tag:#} {name}{range_suffix}")
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
        let status = theme.warning;
        let kind = fetch_content_kind(&tool.arguments);
        return format!("{title}web_fetch{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}");
    }
    let summary = format_tool_args_summary(&tool.name, &tool.arguments);
    if summary.is_empty() {
        format!("{title}{display_name}{title:#}")
    } else {
        let header_args = if tool.name == "bash" {
            format_bash_args_header(&summary, accent, theme.dimmed)
        } else {
            format!("{accent}{summary}{accent:#}")
        };
        format!("{title}{display_name}{title:#} {header_args}")
    }
}

fn append_read_expanded(content: &mut String, tool: &ToolLine, theme: &Theme) {
    let raw = if !tool.output.is_empty() {
        &tool.output
    } else {
        &tool.output_summary
    };
    if let Some(formatted) = format_read_expanded(raw, &tool.arguments, theme) {
        content.push_str("\n\n");
        content.push_str(&formatted);
    }
}

fn append_output_lines(content: &mut String, clean: &str, width: usize, expanded: bool, dim: anstyle::Style) {
    content.push_str("\n\n");
    if expanded {
        content.push_str(clean);
    } else {
        let truncated = truncate_to_visual_lines(clean, 5, width.saturating_sub(4).max(1));
        if truncated.skipped_count > 0 {
            content.push_str(&format!(
                "{dim}... ({n} earlier lines){dim:#}\n",
                n = truncated.skipped_count
            ));
        }
        content.push_str(&truncated.visual_lines.join("\n"));
    }
}

fn append_generic_output(content: &mut String, tool: &ToolLine, width: usize, expanded: bool, dim: anstyle::Style) {
    let raw = if !tool.output.is_empty() {
        &tool.output
    } else {
        &tool.output_summary
    };
    let clean = raw.trim_end().replace('\t', "   ");
    if !clean.is_empty() {
        append_output_lines(content, &clean, width, expanded, dim);
    }
}

fn append_edit_or_write(content: &mut String, tool: &ToolLine, expanded: bool, theme: &Theme) -> bool {
    if !tool.is_error && tool.name == "edit" {
        if let Some(diff) = format_edit_diff(&tool.arguments, theme) {
            content.push_str("\n\n");
            content.push_str(&diff);
        }
        return true;
    }
    if !tool.is_error && tool.name == "write" {
        if let Some(preview) = format_write_preview(&tool.arguments, theme, expanded) {
            content.push_str("\n\n");
            content.push_str(&preview);
        }
        return true;
    }
    false
}

pub fn append_tool_details(
    content: &mut String,
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) {
    if !tool.is_error && tool.name == "read" {
        if show_read_expanded {
            append_read_expanded(content, tool, theme);
        }
    } else if !append_edit_or_write(content, tool, expanded, theme)
        && (tool.name == "bash" || tool.is_error || expanded)
    {
        append_generic_output(content, tool, width, expanded, theme.dimmed);
    }
    if tool.name == "bash"
        && let Some(duration_ms) = tool.duration_ms
    {
        let dim = theme.dimmed;
        content.push_str(&format!(
            "\n\n{dim}Took {}{dim:#}",
            super::format_duration_ms(duration_ms)
        ));
    }
}

pub fn render_tool_block(
    tool: &ToolLine,
    width: usize,
    expanded: bool,
    show_read_expanded: bool,
    theme: &Theme,
) -> String {
    let mut content = format_tool_header(tool, theme);
    append_tool_details(&mut content, tool, width, expanded, show_read_expanded, theme);
    theme
        .tool_block(tool.name == "bash", tool.is_error, width)
        .with_vertical_padding()
        .render_styled(&content)
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

pub fn render_headless_tool_card(line: &ToolLine, theme: &Theme) -> String {
    render_tool_transcript(line, terminal_width(), true, false, theme)
}
