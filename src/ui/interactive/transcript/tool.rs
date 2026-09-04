use crate::ui::block::BlockFormat;
use crate::ui::render::{
    detect_language_from_args, fetch_content_kind, format_duration_ms, format_edit_diff, format_tool_args_summary,
    format_write_preview, read_summary_parts, tool_title_style,
};

use super::types::{ToolItem, TranscriptRenderInput};

pub fn render_tool_transcript(tool: &ToolItem, input: &TranscriptRenderInput<'_>) -> String {
    let theme = input.theme;
    let width = input.width;
    let tools_expanded = input.tools_expanded;

    let background = if tool.is_error {
        theme.tool_error_bg
    } else {
        theme.tool_success_bg
    };
    let title = tool_title_style(tool.is_error);
    let accent = theme.highlight;
    let display_name = match tool.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
    let summary = format_tool_args_summary(&tool.name, &tool.arguments);

    let mut content = if tool.name == "read" && !tool.is_error {
        let (path, range) = read_summary_parts(&tool.arguments);
        let range_suffix = range.map_or_else(String::new, |range| {
            let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            format!("{range_style}{range}{range_style:#}")
        });
        match rho_harness_core::presentation::summary::classify_read_path(&tool.arguments) {
            Some(rho_harness_core::presentation::summary::ReadClassification::Skill { name }) => {
                let skill_tag = anstyle::Style::new()
                    .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
                    .effects(anstyle::Effects::BOLD);
                format!("{skill_tag}[skill]{skill_tag:#} {name}{range_suffix}")
            }
            Some(rho_harness_core::presentation::summary::ReadClassification::Resource { path }) => {
                format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
            Some(rho_harness_core::presentation::summary::ReadClassification::Docs { path }) => {
                format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
            None => {
                format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
        }
    } else if display_name == "web_fetch" && !tool.is_error {
        let url = tool
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let kind = fetch_content_kind(&tool.arguments);
        format!("{title}web_fetch{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}")
    } else {
        format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
    };

    if !tool.is_error && tool.name == "read" && tools_expanded {
        let raw_output = if !tool.output.is_empty() {
            &tool.output
        } else {
            &tool.output_summary
        };
        let clean = raw_output.trim_end();
        if !clean.is_empty() {
            content.push_str("\n\n");
            let lang = detect_language_from_args(&tool.arguments);
            let highlighted_lines: Vec<String> = clean
                .lines()
                .map(|line| {
                    let no_tabs = line.replace('\t', "   ");
                    crate::ui::markdown::highlight_code_line(&no_tabs, lang, theme)
                })
                .collect();
            content.push_str(&highlighted_lines.join("\n"));
        }
    } else if !tool.is_error && tool.name == "edit" {
        if let Some(diff) = format_edit_diff(&tool.arguments, theme) {
            content.push_str("\n\n");
            content.push_str(&diff);
        }
    } else if !tool.is_error && tool.name == "write" {
        if let Some(preview) = format_write_preview(&tool.arguments, theme, tools_expanded) {
            content.push_str("\n\n");
            content.push_str(&preview);
        }
    } else if tool.name == "bash" || tool.is_error || (tools_expanded && tool.name != "edit" && tool.name != "write") {
        let raw_output = if !tool.output.is_empty() {
            &tool.output
        } else {
            &tool.output_summary
        };
        // Tabs count as zero width here but expand to tab stops on screen,
        // desyncing block background fill and wrap math.
        let clean = raw_output.trim_end().replace('\t', "   ");
        if !clean.is_empty() {
            content.push_str("\n\n");
            if tools_expanded {
                content.push_str(&clean);
            } else {
                let truncated =
                    crate::ui::interactive::layout::truncate_to_visual_lines(&clean, 5, width.saturating_sub(4).max(1));
                if truncated.skipped_count > 0 {
                    let dim = theme.dimmed;
                    content.push_str(&format!(
                        "{dim}... ({} earlier lines){dim:#}\n",
                        truncated.skipped_count
                    ));
                }
                content.push_str(&truncated.visual_lines.join("\n"));
            }
        }
    }

    if tool.name == "bash"
        && let Some(duration_ms) = tool.duration_ms
    {
        let dim = theme.dimmed;
        content.push_str("\n\n");
        content.push_str(&format!("{dim}Took {}{dim:#}", format_duration_ms(duration_ms)));
    }

    let block = BlockFormat::new(background, width)
        .with_vertical_padding()
        .render_styled(&content);
    format!("\n{block}")
}
