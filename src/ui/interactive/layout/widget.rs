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
    let width = input.width.max(20);
    let title = tool_title_style(false);
    let accent = input.theme.highlight;
    let dim = input.theme.dimmed;

    let display_name = match input.tool.name.as_str() {
        "web_search" | "websearch" => "search",
        "web_fetch" | "webfetch" => "fetch",
        other => other,
    };

    let mut content = format!(
        "{title}{display_name}{title:#} {accent}{}{accent:#}",
        input.tool.args_summary
    );

    if let Some(preview) = &input.tool.preview {
        content.push('\n');
        content.push_str(preview);
    }

    // Tabs count as zero width here but expand to tab stops on screen,
    // desyncing block background fill and wrap math.
    let raw_output = input.tool.output.trim_end().replace('\t', "   ");
    if !raw_output.is_empty() {
        content.push_str("\n\n");
        if input.tools_expanded {
            content.push_str(&raw_output);
        } else {
            let truncated = truncate_to_visual_lines(&raw_output, 5, width.saturating_sub(4).max(1));
            if truncated.skipped_count > 0 {
                content.push_str(&format!(
                    "{dim}... ({} earlier lines, Ctrl+O to expand){dim:#}\n",
                    truncated.skipped_count
                ));
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

    block.lines().map(|s| s.to_string()).collect()
}
