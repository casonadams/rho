use super::text::truncate_to_visual_lines;
use crate::ui::block::BlockFormat;
use crate::ui::interactive::state::RunningTool;
use crate::ui::render::tool_title_style;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy)]
pub struct RunningToolWidgetInput<'a> {
    pub tool: &'a RunningTool,
    pub theme: &'a Theme,
    pub width: usize,
    pub tools_expanded: bool,
}

pub fn render_running_tool_widget(input: RunningToolWidgetInput<'_>) -> Vec<String> {
    if input.tool.preview.is_none()
        && input.tool.output.is_empty()
        && input.tool.name != "bash"
        && input.tool.name != "write"
    {
        return Vec::new();
    }

    let width = input.width.max(20);
    let title = tool_title_style(false);
    let accent = input.theme.highlight;
    let dim = input.theme.dimmed;

    let display_name = match input.tool.name.as_str() {
        "search" | "websearch" => "web_search",
        "fetch" | "webfetch" => "web_fetch",
        other => other,
    };

    let mut content = format!(
        "{title}{display_name}{title:#} {accent}{}{accent:#}",
        input.tool.args_summary
    );

    if let Some(preview) = &input.tool.preview {
        content.push_str("\n\n");
        content.push_str(preview);
    }

    // Tabs count as zero width here but expand to tab stops on screen,
    // desyncing block background fill and wrap math.
    let raw_output = input.tool.output.trim_end().replace('\t', "   ");
    if !raw_output.is_empty() {
        content.push_str("\n\n");
        if input.tool.name == "write" {
            let lang = crate::ui::render::preview::detect_language_from_path(&input.tool.args_summary);
            let lines: Vec<&str> = raw_output.lines().collect();
            let total = lines.len();
            let max = if input.tools_expanded { total } else { 10.min(total) };
            let gutter_width = max.to_string().len().max(3);
            for (idx, line) in lines[..max].iter().enumerate() {
                let line_num = idx + 1;
                let highlighted = crate::ui::markdown::highlight_code_line(line, lang, input.theme);
                content.push_str(&format!("{dim}{line_num:>gutter_width$} │ {dim:#}{highlighted}\n"));
            }
            if !input.tools_expanded && total > 10 {
                let dim = input.theme.dimmed;
                content.push_str(&format!("{dim}... ({} more lines, {total} total){dim:#}\n", total - 10));
            }
        } else if input.tools_expanded {
            content.push_str(&raw_output);
        } else {
            const PRE_SLICE_LINE_LIMIT: usize = 50;
            let total_lines = raw_output.bytes().filter(|&b| b == b'\n').count() + 1;
            let (tail_text, earlier_skipped) = if total_lines > PRE_SLICE_LINE_LIMIT {
                if let Some((idx, _)) = raw_output.rmatch_indices('\n').nth(PRE_SLICE_LINE_LIMIT - 1) {
                    (&raw_output[idx + 1..], total_lines - PRE_SLICE_LINE_LIMIT)
                } else {
                    (raw_output.as_str(), 0)
                }
            } else {
                (raw_output.as_str(), 0)
            };

            let truncated = truncate_to_visual_lines(tail_text, 5, width.saturating_sub(4).max(1));
            let total_skipped = earlier_skipped + truncated.skipped_count;
            if total_skipped > 0 {
                content.push_str(&format!("{dim}... ({total_skipped} earlier lines){dim:#}\n"));
            }
            content.push_str(&truncated.visual_lines.join("\n"));
        }
    }

    let elapsed = input.tool.elapsed();
    let elapsed_str = if elapsed.as_secs() > 0 {
        format!("{:.1}s", elapsed.as_secs_f64())
    } else {
        format!("{}ms", elapsed.as_millis())
    };
    content.push_str(&format!("\n\n{dim}Elapsed {elapsed_str}{dim:#}"));

    let block = BlockFormat::new(input.theme.tool_success_bg, width)
        .with_vertical_padding()
        .render_styled(&content);

    let mut lines = vec![String::new()];
    lines.extend(block.lines().map(|s| s.to_string()));
    lines
}
