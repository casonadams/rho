use super::formatters::{format_edit_diff, format_write_preview};
use super::preview::{fetch_content_kind, tool_title_style};
use crate::ui::block::{BlockFormat, terminal_width};
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_tool_args_summary, read_summary_parts,
};

/// Formats a tool completion line into a bordered terminal card for non-interactive rendering.
pub(crate) fn render_headless_tool_card(line: &ToolLine, theme: &Theme) -> String {
    let background = if line.is_error {
        theme.tool_error_bg
    } else {
        theme.tool_success_bg
    };
    let title = tool_title_style(line.is_error);
    let accent = theme.highlight;
    let summary = format_tool_args_summary(&line.name, &line.arguments);
    let display_name = match line.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };
    let mut content = if line.name == "read" && !line.is_error {
        let (path, range) = read_summary_parts(&line.arguments);
        let range_suffix = range.map_or_else(String::new, |range| {
            let range_style = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
            format!("{range_style}{range}{range_style:#}")
        });
        match classify_read_path(&line.arguments) {
            Some(ReadClassification::Skill { name }) => {
                let skill_tag = anstyle::Style::new()
                    .fg_color(Some(anstyle::AnsiColor::Magenta.into()))
                    .effects(anstyle::Effects::BOLD);
                format!("{skill_tag}[skill]{skill_tag:#} {name}{range_suffix}")
            }
            Some(ReadClassification::Resource { path }) => {
                format!("{title}read resource{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
            Some(ReadClassification::Docs { path }) => {
                format!("{title}read docs{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
            None => {
                format!("{title}read{title:#} {accent}{path}{accent:#}{range_suffix}")
            }
        }
    } else if display_name == "web_fetch" && !line.is_error {
        let url = line
            .arguments
            .get("url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let status = anstyle::Style::new().fg_color(Some(anstyle::AnsiColor::Yellow.into()));
        let kind = fetch_content_kind(&line.arguments);
        format!("{title}{display_name}{title:#} {accent}{url}{accent:#}\n{status}fetched ({kind}){status:#}")
    } else {
        format!("{title}{display_name}{title:#} {accent}{summary}{accent:#}")
    };
    if !line.is_error && line.name == "edit" {
        if let Some(diff) = format_edit_diff(&line.arguments, theme) {
            content.push_str("\n\n");
            content.push_str(&diff);
        }
    } else if !line.is_error && line.name == "write" {
        if let Some(preview) = format_write_preview(&line.arguments, theme, true) {
            content.push_str("\n\n");
            content.push_str(&preview);
        }
    } else if line.name == "bash" || line.is_error {
        let raw_output = if !line.output.is_empty() {
            line.output.as_str()
        } else {
            line.output_summary.as_str()
        };
        let clean = raw_output.trim_end();
        if !clean.is_empty() {
            content.push_str("\n\n");
            content.push_str(clean);
        }
    }

    if line.name == "bash"
        && let Some(duration) = line.duration_ms
    {
        let dim = theme.dimmed;
        content.push_str("\n\n");
        content.push_str(&format!("{dim}Took {}{dim:#}", super::format_duration_ms(duration)));
    }

    let block = BlockFormat::new(background, terminal_width())
        .with_vertical_padding()
        .render_styled(&content);
    format!("\n{block}")
}
