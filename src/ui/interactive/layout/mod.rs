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

pub use text::{SPINNER_FRAMES, VisualTruncateResult, truncate_to_visual_lines, wrap_to_width};
pub use types::{CursorPosition, InteractiveLayout, LayoutInput};
pub use widget::{RunningToolWidgetInput, render_running_tool_widget};

use normal::render_normal_layout;

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let mut rendered = render_normal_layout(input);
    let default_theme = crate::ui::theme::Theme::default();
    let theme = input.theme.unwrap_or(&default_theme);
    rendered.lines = crate::ui::interactive::region::paint_lines(&rendered.lines, theme, input.terminal_width);
    rendered.bg = crate::ui::interactive::region::bg_code(theme);
    rendered
}
