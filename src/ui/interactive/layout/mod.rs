pub mod autocomplete;
pub mod budget;
pub mod chrome;
pub mod editor;
pub mod modal;
pub mod normal;
#[cfg(test)]
mod tests;
pub mod text;
pub mod types;
pub mod widget;

pub use modal::modal_body_max_scroll;
pub use text::{SPINNER_FRAMES, VisualTruncateResult, truncate_to_visual_lines, wrap_to_width, wrap_words_to_width};
pub use types::{CursorPosition, InteractiveLayout, LayoutInput};
pub use widget::{RunningToolWidgetInput, render_running_tool_widget};

use normal::render_normal_layout;

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    render_normal_layout(input)
}
