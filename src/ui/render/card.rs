use super::formatters::{format_edit_diff, format_write_preview};
use crate::ui::block::terminal_width;
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;
use rho_harness_core::presentation::summary::{
    ReadClassification, classify_read_path, format_tool_args_summary, read_summary_parts,
};

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
    if !line.is_error && line.name == "read" {
        return format_read_header(line, theme);
    }
    let summary = format_tool_args_summary(&line.name, &line.arguments);
    if summary.is_empty() {
        format!("{title}{}{title:#}", line.name)
    } else {
        let header_args = if line.name == "bash" {
            format_bash_args_header(&summary, accent, theme.dimmed)
        } else {
            format!("{accent}{summary}{accent:#}")
        };
        format!("{title}{}{title:#} {header_args}", line.name)
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
