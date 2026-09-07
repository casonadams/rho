use crate::ui::block::BlockFormat;
use crate::ui::render::{
    detect_language_from_args, fetch_content_kind, format_duration_ms, format_edit_diff, format_tool_args_summary,
    format_write_preview, read_summary_parts,
};

use super::types::{ToolItem, TranscriptRenderInput};

fn format_read_header(tool: &ToolItem, theme: &crate::ui::theme::Theme) -> String {
    let (path, range) = read_summary_parts(&tool.arguments);
    let title = theme.tool_title_style(false);
    let accent = theme.highlight;
    let range_suffix = range.map_or_else(String::new, |range| {
        let rs = theme.warning;
        format!("{rs}{range}{rs:#}")
    });
    match rho_harness_core::presentation::summary::classify_read_path(&tool.arguments) {
        Some(rho_harness_core::presentation::summary::ReadClassification::Skill { name }) => {
            let tag = theme.skill_tag;
            format!("{tag}[skill]{tag:#} {name}{range_suffix}")
        }
        Some(rho_harness_core::presentation::summary::ReadClassification::Resource { path }) => {
            format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        Some(rho_harness_core::presentation::summary::ReadClassification::Docs { path }) => {
            format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
        }
        None => format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}"),
    }
}

fn format_tool_header(tool: &ToolItem, theme: &crate::ui::theme::Theme) -> String {
    let title = theme.tool_title_style(tool.is_error);
    let accent = theme.highlight;
    let display_name = match tool.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
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
    format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
}

fn append_read_expanded(content: &mut String, tool: &ToolItem, theme: &crate::ui::theme::Theme) {
    let raw = if !tool.output.is_empty() {
        &tool.output
    } else {
        &tool.output_summary
    };
    let clean = raw.trim_end();
    if clean.is_empty() {
        return;
    }
    content.push_str("\n\n");
    let lang = detect_language_from_args(&tool.arguments);
    let highlighted: Vec<String> = clean
        .lines()
        .map(|l| crate::ui::markdown::highlight_code_line(&l.replace('\t', "   "), lang, theme))
        .collect();
    content.push_str(&highlighted.join("\n"));
}

fn append_output_lines(content: &mut String, clean: &str, width: usize, expanded: bool, dim: anstyle::Style) {
    content.push_str("\n\n");
    if expanded {
        content.push_str(clean);
    } else {
        let truncated =
            crate::ui::interactive::layout::truncate_to_visual_lines(clean, 5, width.saturating_sub(4).max(1));
        if truncated.skipped_count > 0 {
            content.push_str(&format!(
                "{dim}... ({n} earlier lines){dim:#}\n",
                n = truncated.skipped_count
            ));
        }
        content.push_str(&truncated.visual_lines.join("\n"));
    }
}

fn append_generic_output(content: &mut String, tool: &ToolItem, width: usize, expanded: bool, dim: anstyle::Style) {
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

fn append_edit_or_write(
    content: &mut String,
    tool: &ToolItem,
    expanded: bool,
    theme: &crate::ui::theme::Theme,
) -> bool {
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

fn append_tool_details(
    content: &mut String,
    tool: &ToolItem,
    width: usize,
    expanded: bool,
    theme: &crate::ui::theme::Theme,
) {
    if !tool.is_error && tool.name == "read" {
        if expanded {
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
        content.push_str(&format!("\n\n{dim}Took {}{dim:#}", format_duration_ms(duration_ms)));
    }
}

pub fn render_tool_block(tool: &ToolItem, input: &TranscriptRenderInput<'_>) -> String {
    let background = if tool.is_error {
        input.theme.tool_error_bg
    } else {
        input.theme.tool_success_bg
    };
    let mut content = format_tool_header(tool, input.theme);
    append_tool_details(&mut content, tool, input.width, input.tools_expanded, input.theme);

    BlockFormat::new(background, input.width)
        .with_vertical_padding()
        .render_styled(&content)
}

pub fn render_tool_transcript(tool: &ToolItem, input: &TranscriptRenderInput<'_>) -> String {
    let block = render_tool_block(tool, input);
    format!("\n{block}")
}
