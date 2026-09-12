use super::text::truncate_to_visual_lines;
use crate::ui::interactive::state::RunningTool;
use crate::ui::render::{format_bash_args_header, tool_title_style};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct RunningToolWidgetInput<'a> {
    pub tool: &'a RunningTool,
    pub theme: &'a Theme,
    pub width: usize,
    pub tools_expanded: bool,
}

fn normalize_tool_name(name: &str) -> &str {
    match name {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    }
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

fn format_widget_content(input: RunningToolWidgetInput<'_>, width: usize) -> String {
    let title = tool_title_style(false);
    let (accent, dim) = (input.theme.highlight, input.theme.dimmed);
    let display_name = normalize_tool_name(&input.tool.name);

    let args_header = if input.tool.name == "bash" {
        format_bash_args_header(&input.tool.args_summary, accent, dim)
    } else {
        format!("{accent}{}{accent:#}", input.tool.args_summary)
    };

    let mut content = format!("{title}{display_name}{title:#} {args_header}");
    if let Some(preview) = &input.tool.preview {
        content.push_str("\n\n");
        content.push_str(preview);
    }
    let raw_output = input.tool.output.trim_end().replace('\t', "   ");
    append_tool_output(&mut content, &raw_output, input.tools_expanded, width, dim);
    content.push_str(&format!(
        "\n\n{dim}Elapsed {}{dim:#}",
        format_elapsed(input.tool.elapsed())
    ));
    content
}

pub fn render_running_tool_widget(input: RunningToolWidgetInput<'_>) -> Vec<String> {
    if input.tool.preview.is_none() && input.tool.output.is_empty() && input.tool.name != "bash" {
        return Vec::new();
    }
    let width = input.width.max(20);
    let content = format_widget_content(input, width);
    let block = input
        .theme
        .tool_block(input.tool.name == "bash", false, width)
        .with_vertical_padding()
        .render_styled(&content);
    let mut lines = if input.theme.block_style == crate::ui::theme::BlockStyle::Border {
        Vec::new()
    } else {
        vec![String::new()]
    };
    lines.extend(block.lines().map(String::from));
    lines
}
