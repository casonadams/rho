use crate::ui::interactive::state::RunningTool;
use crate::ui::render::card::render_running_tool_block;
use crate::ui::theme::{BlockStyle, Theme};
use rho_harness_core::presentation::ToolLine;

#[derive(Debug, Clone, Copy)]
pub struct RunningToolWidgetInput<'a> {
    pub tool: &'a RunningTool,
    pub theme: &'a Theme,
    pub width: usize,
    pub tools_expanded: bool,
}

/// The arguments the card is drawn from; a running `write` shows its streamed content as the text.
fn live_arguments(tool: &RunningTool) -> serde_json::Value {
    let mut arguments = tool.arguments.clone();
    if tool.name == "write"
        && !tool.output.is_empty()
        && let Some(object) = arguments.as_object_mut()
    {
        object.insert("text".to_string(), serde_json::Value::String(tool.output.clone()));
    }
    arguments
}

fn running_tool_line(tool: &RunningTool) -> ToolLine {
    let mut line = ToolLine::new(tool.name.as_str(), live_arguments(tool))
        .with_output(tool.output.clone())
        .with_duration_ms(tool.elapsed().as_millis() as u64);
    line.file_change = tool.file_change.clone();
    line
}

pub fn render_running_tool_widget(input: RunningToolWidgetInput<'_>) -> Vec<String> {
    if input.tool.output.is_empty() && !RunningTool::has_live_card(&input.tool.name) {
        return Vec::new();
    }
    let width = input.width.max(20);
    let block = render_running_tool_block(&running_tool_line(input.tool), width, input.tools_expanded, input.theme);
    let mut lines = if input.theme.block_style == BlockStyle::Border {
        Vec::new()
    } else {
        vec![String::new()]
    };
    lines.extend(block.lines().map(String::from));
    lines
}
