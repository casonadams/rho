use super::text::truncate_to_visual_lines;

#[derive(Debug, Clone, Copy)]
pub struct ActiveToolDisplayInput<'a> {
    pub tool_name: &'a str,
    pub args_summary: &'a str,
    pub preview: Option<&'a str>,
    pub output: &'a str,
    pub started: std::time::Instant,
    pub theme: &'a crate::ui::theme::Theme,
    pub width: usize,
    pub expanded: bool,
}

pub fn format_active_tool_block(input: ActiveToolDisplayInput<'_>) -> String {
    let width = input.width.max(20);
    let title_style = input.theme.tool_header;
    let accent_style = input.theme.highlight;
    let dim_style = input.theme.dimmed;

    let header = format!(
        "{title_style}{}{title_style:#} {accent_style}{}{accent_style:#}",
        input.tool_name, input.args_summary
    );
    let mut content = header;

    if let Some(preview) = input.preview
        && !preview.trim().is_empty()
    {
        content.push('\n');
        content.push_str(preview);
    }

    let clean_output = input.output.trim_end();
    if !clean_output.is_empty() {
        content.push('\n');
        if input.expanded {
            for line in clean_output.lines() {
                content.push('\n');
                content.push_str(&format!("{dim_style}{line}{dim_style:#}"));
            }
        } else {
            let truncated = truncate_to_visual_lines(clean_output, 5, width.saturating_sub(4).max(1));
            if truncated.skipped_count > 0 {
                content.push('\n');
                content.push_str(&format!(
                    "{dim_style}... ({} earlier lines, Ctrl+O to expand){dim_style:#}",
                    truncated.skipped_count
                ));
            }
            for line in truncated.visual_lines {
                content.push('\n');
                content.push_str(&format!("{dim_style}{line}{dim_style:#}"));
            }
        }
    }

    let elapsed = input.started.elapsed();
    let elapsed_text = format!("Elapsed {}", crate::ui::render::format_duration(elapsed));
    content.push('\n');
    content.push_str(&format!("{dim_style}{elapsed_text}{dim_style:#}"));

    crate::ui::block::BlockFormat::new(input.theme.tool_success_bg, width)
        .with_vertical_padding()
        .render_styled(&content)
}
