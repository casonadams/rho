use super::formatters::{format_edit_diff, format_write_preview};
use crate::ui::block::terminal_width;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_tool_args_summary, read_summary_parts,
};

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

pub fn format_bash_args_header(summary: &str, accent: crate::ui::theme::Style, dim: crate::ui::theme::Style) -> String {
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

fn format_read_header(line: &ToolLine, theme: &Theme) -> String {
    let (path, range) = read_summary_parts(&line.arguments);
    let title = theme.tool_title_style(false);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |range| {
        let rs = theme.warning;
        format!("{rs}{range}{rs:#}")
    });
    match classify_read_path(&line.arguments) {
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

fn format_card_header(line: &ToolLine, theme: &Theme) -> String {
    let title = theme.tool_title_style(line.is_error);
    let accent = theme.highlight;
    let display_name = match line.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
    if !line.is_error && line.name == "read" {
        return format_read_header(line, theme);
    }
    if !line.is_error && display_name == "web_fetch" {
        let url = line
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let status = theme.warning;
        let kind = fetch_content_kind(&line.arguments);
        return format!("{title}{display_name}{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}");
    }
    let summary = format_tool_args_summary(&line.name, &line.arguments);
    if summary.is_empty() {
        format!("{title}{display_name}{title:#}")
    } else {
        let header_args = if line.name == "bash" {
            format_bash_args_header(&summary, accent, theme.dimmed)
        } else {
            format!("{accent}{summary}{accent:#}")
        };
        format!("{title}{display_name}{title:#} {header_args}")
    }
}

fn append_edit_write(content: &mut String, line: &ToolLine, theme: &Theme) -> bool {
    if !line.is_error && line.name == "edit" {
        if let Some(diff) = format_edit_diff(&line.arguments, theme) {
            content.push_str("\n\n");
            content.push_str(&diff);
        }
        return true;
    }
    if !line.is_error && line.name == "write" {
        if let Some(preview) = format_write_preview(&line.arguments, theme, true) {
            content.push_str("\n\n");
            content.push_str(&preview);
        }
        return true;
    }
    false
}

fn append_card_body(content: &mut String, line: &ToolLine, theme: &Theme) {
    if !append_edit_write(content, line, theme) && (line.name == "bash" || line.is_error) {
        let raw = if !line.output.is_empty() {
            &line.output
        } else {
            &line.output_summary
        };
        let clean = raw.trim_end();
        if !clean.is_empty() {
            content.push_str("\n\n");
            content.push_str(clean);
        }
    }
    if line.name == "bash"
        && let Some(duration) = line.duration_ms
    {
        let dim = theme.dimmed;
        content.push_str(&format!("\n\n{dim}Took {}{dim:#}", super::format_duration_ms(duration)));
    }
}

pub(crate) fn render_headless_tool_card(line: &ToolLine, theme: &Theme) -> String {
    let mut content = format_card_header(line, theme);
    append_card_body(&mut content, line, theme);
    let block = theme
        .tool_block(line.name == "bash", line.is_error, terminal_width())
        .with_vertical_padding()
        .render_styled(&content);
    if theme.block_style == crate::ui::theme::BlockStyle::Border {
        block
    } else {
        format!("\n{block}")
    }
}
