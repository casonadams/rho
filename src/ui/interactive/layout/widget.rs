use crate::ui::interactive::state::{LiveCardView, RunningTool};
use crate::ui::render::live_card::LiveCard;
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
    match tool.arguments.as_object() {
        Some(object) if tool.name == "write" && !tool.output.is_empty() => {
            let mut arguments: serde_json::Map<String, serde_json::Value> = object
                .iter()
                .map(|(key, value)| {
                    let value = if key == "text" {
                        serde_json::Value::Null
                    } else {
                        value.clone()
                    };
                    (key.clone(), value)
                })
                .collect();
            arguments.insert("text".to_string(), serde_json::Value::String(tool.output.clone()));
            serde_json::Value::Object(arguments)
        }
        _ => tool.arguments.clone(),
    }
}

/// The output the card is drawn from; a running `bash` shows it tidied like its finished card will.
fn live_output(tool: &RunningTool) -> String {
    if tool.name == "bash" {
        rho_engine::tools::bash::normalize::tidy_output(&tool.output)
    } else {
        tool.output.clone()
    }
}

fn running_tool_line(tool: &RunningTool) -> ToolLine {
    let mut line = ToolLine::new(tool.name.as_str(), live_arguments(tool)).with_output(live_output(tool));
    line.file_change = tool.file_change.clone();
    line
}

pub fn render_running_tool_widget(input: RunningToolWidgetInput<'_>) -> Vec<String> {
    if input.tool.output.is_empty() && !RunningTool::has_live_card(&input.tool.name) {
        return Vec::new();
    }
    let width = input.width.max(20);
    let view = LiveCardView {
        width,
        expanded: input.tools_expanded,
        theme: input.theme.clone(),
    };
    let card = input.tool.live_card(view, || {
        LiveCard::new(
            &running_tool_line(input.tool),
            input.tool.dropped_lines,
            width,
            input.tools_expanded,
            input.theme,
        )
    });
    let block = card.render(input.tool.elapsed().as_millis() as u64, input.theme);
    let mut lines = if input.theme.block_style == BlockStyle::Border {
        Vec::new()
    } else {
        vec![String::new()]
    };
    lines.extend(block.lines().map(String::from));
    lines
}
