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

use modal::render_modal_overlay;
use normal::render_normal_layout;

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let mut rendered = if let Some(modal) = input.modal {
        render_modal_layout(modal, &input)
    } else {
        render_normal_layout(input)
    };
    let default_theme = crate::ui::theme::Theme::default();
    let theme = input.theme.unwrap_or(&default_theme);
    rendered.lines = crate::ui::interactive::region::paint_lines(&rendered.lines, theme, input.terminal_width);
    rendered.bg = crate::ui::interactive::region::bg_code(theme);
    rendered
}

fn render_modal_layout(modal: &crate::ui::interactive::ModalState, input: &LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let (modal_lines, modal_cursor, modal_cursor_visible) =
        render_modal_overlay(modal, (width, input.terminal_height), input.theme);
    let cursor_row = modal_cursor.row;

    InteractiveLayout {
        lines: modal_lines.clone(),
        bg: String::new(),
        cursor: modal_cursor,
        cursor_visible: modal_cursor_visible,
        cursor_row,
        queued_lines: Vec::new(),
        widget_lines: Vec::new(),
        working_line: String::new(),
        top_divider: String::new(),
        editor_lines: modal_lines,
        bottom_divider: String::new(),
        footer_lines: Vec::new(),
        footer: String::new(),
    }
}
