use super::text::truncate_to_visual_lines;
use crate::ui::interactive::state::RunningTool;
use crate::ui::render::card::{frame_tool_card, render_tool_block, style_tool_header_args};
use crate::ui::render::structured::{
    align_header_with_right, append_section, format_structured_bash_input, format_structured_header_left,
};
use crate::ui::render::{format_bash_args_header, highlight_bash_args_header, normalize_tool_name, tool_title_style};
use crate::ui::theme::Theme;
use rho_harness_core::presentation::ToolLine;

#[derive(Debug, Clone, Copy)]
pub struct RunningToolWidgetInput<'a> {
    pub tool: &'a RunningTool,
    pub theme: &'a Theme,
    pub width: usize,
    pub tools_expanded: bool,
}

fn format_elapsed(duration: std::time::Duration) -> String {
    if duration.as_secs() > 0 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

fn slice_tail_lines(raw: &str, limit: usize) -> (&str, usize) {
    let total = raw.bytes().filter(|&b| b == b'\n').count() + 1;
    if total > limit
        && let Some((idx, _)) = raw.rmatch_indices('\n').nth(limit - 1)
    {
        (&raw[idx + 1..], total - limit)
    } else {
        (raw, 0)
    }
}

fn format_collapsed_output(raw_output: &str, width: usize, dim: anstyle::Style) -> String {
    const PRE_SLICE_LINE_LIMIT: usize = 50;
    let (tail_text, earlier_skipped) = slice_tail_lines(raw_output, PRE_SLICE_LINE_LIMIT);
    let truncated = truncate_to_visual_lines(tail_text, 5, width.saturating_sub(4).max(1));
    let total_skipped = earlier_skipped + truncated.skipped_count;
    let mut out = String::new();
    if total_skipped > 0 {
        out.push_str(&format!("{dim}... ({total_skipped} earlier lines){dim:#}\n"));
    }
    out.push_str(&truncated.visual_lines.join("\n"));
    out
}

fn append_tool_output(content: &mut String, raw_output: &str, expanded: bool, width: usize, dim: anstyle::Style) {
    if raw_output.is_empty() {
        return;
    }
    content.push_str("\n\n");
    if expanded {
        content.push_str(raw_output);
    } else {
        content.push_str(&format_collapsed_output(raw_output, width, dim));
    }
}

/// A running `write` is drawn by the committed-card renderer, so finishing it changes nothing but the duration.
fn running_write_block(input: &RunningToolWidgetInput<'_>, width: usize) -> String {
    let path = input.tool.args_summary.split(' ').next().unwrap_or_default();
    let mut line = ToolLine::new("write", serde_json::json!({ "path": path, "text": input.tool.output }))
        .with_duration_ms(input.tool.elapsed().as_millis() as u64);
    line.file_change = input.tool.file_change.clone();
    render_tool_block(&line, width, input.tools_expanded, false, input.theme)
}

fn highlight_live_output(tool: &RunningTool, raw_output: String, theme: &Theme) -> String {
    if !theme.syntax_highlight_tools {
        return raw_output;
    }
    crate::ui::render::output_highlight::highlight_tool_output(&tool.name, &raw_output, theme)
}

fn format_widget_content(input: RunningToolWidgetInput<'_>, width: usize) -> String {
    let title = tool_title_style(false);
    let (accent, dim) = (input.theme.highlight, input.theme.dimmed);
    let display_name = normalize_tool_name(&input.tool.name);

    let args_header = if input.tool.name == "bash" {
        if input.theme.syntax_highlight_tools {
            highlight_bash_args_header(&input.tool.args_summary, input.theme)
        } else {
            format_bash_args_header(&input.tool.args_summary, accent, dim)
        }
    } else if input.theme.syntax_highlight_tools
        && (input.tool.args_summary.starts_with('{') || input.tool.args_summary.starts_with('['))
    {
        crate::ui::markdown::highlight_code_line(&input.tool.args_summary, Some("json"), input.theme)
    } else {
        format!("{accent}{}{accent:#}", input.tool.args_summary)
    };

    let mut content = format!("{title}{display_name}{title:#} {args_header}");
    if let Some(preview) = &input.tool.preview {
        content.push_str("\n\n");
        content.push_str(preview);
    }
    let raw_output = input.tool.output.trim_end().replace('\t', "   ");
    let rendered = highlight_live_output(input.tool, raw_output, input.theme);
    append_tool_output(&mut content, &rendered, input.tools_expanded, width, dim);
    content.push_str(&format!(
        "\n\n{dim}Elapsed {}{dim:#}",
        format_elapsed(input.tool.elapsed())
    ));
    content
}

fn format_structured_widget_content(input: RunningToolWidgetInput<'_>, width: usize) -> String {
    let inner_width = input
        .theme
        .tool_block(input.tool.name == "bash", false, width)
        .inner_width();
    let (input_body, meta) = if input.tool.name == "bash" {
        format_structured_bash_input(&input.tool.args_summary, input.theme)
    } else if input.tool.args_summary.is_empty() {
        (String::new(), None)
    } else {
        (
            style_tool_header_args(&input.tool.name, &input.tool.args_summary, input.theme),
            None,
        )
    };
    let left = format_structured_header_left(
        &input.tool.name,
        "\u{25cf}",
        input.theme.dimmed,
        false,
        meta.as_deref(),
        input.theme,
    );
    let elapsed = format_elapsed(input.tool.elapsed());
    let mut content = align_header_with_right(&left, Some(&elapsed), inner_width, input.theme.dimmed);
    append_section(&mut content, "Input", &input_body, input.theme.dimmed);

    let mut output_parts = Vec::new();
    if let Some(preview) = &input.tool.preview {
        output_parts.push(preview.clone());
    }
    let raw_output = input.tool.output.trim_end().replace('\t', "   ");
    if !raw_output.is_empty() {
        let rendered = highlight_live_output(input.tool, raw_output, input.theme);
        let formatted = if input.tools_expanded {
            rendered
        } else {
            format_collapsed_output(&rendered, width, input.theme.dimmed)
        };
        output_parts.push(formatted);
    }
    append_section(&mut content, "Output", &output_parts.join("\n\n"), input.theme.dimmed);
    content
}

pub fn render_running_tool_widget(input: RunningToolWidgetInput<'_>) -> Vec<String> {
    if input.tool.preview.is_none() && input.tool.output.is_empty() && !RunningTool::has_live_card(&input.tool.name) {
        return Vec::new();
    }
    let width = input.width.max(20);
    let block = if input.tool.name == "write" {
        running_write_block(&input, width)
    } else {
        let content = if input.theme.structured_tool_cards {
            format_structured_widget_content(input, width)
        } else {
            format_widget_content(input, width)
        };
        frame_tool_card(&content, input.tool.name == "bash", false, width, input.theme)
    };
    let mut lines = if input.theme.block_style == crate::ui::theme::BlockStyle::Border {
        Vec::new()
    } else {
        vec![String::new()]
    };
    lines.extend(block.lines().map(String::from));
    lines
}
