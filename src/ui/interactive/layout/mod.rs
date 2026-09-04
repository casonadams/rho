pub mod autocomplete;
pub mod chrome;
pub mod editor;
pub mod modal;
#[cfg(test)]
mod tests;
pub mod text;
pub mod types;
pub mod widget;

pub use chrome::thinking_divider_style;
pub use text::{SPINNER_FRAMES, VisualTruncateResult, truncate_to_visual_lines, wrap_to_width};
pub use types::{CursorPosition, InteractiveLayout, LayoutInput};
pub use widget::{RunningToolWidgetInput, render_running_tool_widget};

use autocomplete::render_autocomplete_dropdown;
use chrome::{queued_lines_text, top_divider, working_line_text};
use editor::wrap_editor;
use modal::render_modal_overlay;

pub fn layout(input: LayoutInput<'_>) -> InteractiveLayout {
    let width = input.terminal_width.max(1);
    let mut lines = Vec::new();

    let queued_lines = queued_lines_text(input.queued_messages, width);
    lines.extend(queued_lines.clone());

    let (working_line, widget_lines) = if input.modal.is_some() {
        (String::new(), Vec::new())
    } else {
        (
            working_line_text(input.footer, input.spinner_frame, width),
            input.widget_lines.to_vec(),
        )
    };

    if !widget_lines.is_empty() {
        lines.extend(widget_lines.clone());
    }

    let (editor_lines, top_divider, bottom_divider, footer_lines, cursor, cursor_visible, cursor_row) =
        if let Some(modal) = input.modal {
            let modal_start_row = lines.len();
            let (modal_lines, modal_cursor, modal_cursor_visible) = render_modal_overlay(modal, width);
            lines.extend(modal_lines.clone());
            (
                modal_lines,
                String::new(),
                String::new(),
                Vec::new(),
                modal_cursor,
                modal_cursor_visible,
                modal_start_row + modal_cursor.row,
            )
        } else {
            lines.push(String::new());
            if !working_line.is_empty() {
                lines.push(working_line.clone());
            }

            let is_bash_mode = input.editor.text().trim_start().starts_with('!');
            let (style, reset) = if is_bash_mode {
                ("\x1b[33m", "\x1b[0m")
            } else {
                thinking_divider_style(input.footer.thinking_level.as_deref())
            };
            let label = if input.footer.show_label {
                concat!("rho ", env!("CARGO_PKG_VERSION"))
            } else {
                ""
            };
            let top_div = top_divider(width, label, (style, reset));
            lines.push(top_div.clone());

            let (mut ed_lines, ed_cursor) = wrap_editor(input.editor, width);
            if let Some(ac) = input.autocomplete {
                let ac_lines = render_autocomplete_dropdown(ac, width);
                if !ac_lines.is_empty() {
                    ed_lines.extend(ac_lines);
                }
            }
            let editor_start_row = lines.len();
            lines.extend(ed_lines.clone());

            let bot_div = format!("{style}{}{reset}", "─".repeat(width));
            lines.push(bot_div.clone());

            let ft_lines = crate::ui::interactive::footer::format_footer_lines(input.footer, width);
            let footer_style = crate::ui::theme::Theme::default().dimmed;
            for fl in &ft_lines {
                lines.push(format!("{footer_style}{fl}{footer_style:#}"));
            }
            (
                ed_lines,
                top_div,
                bot_div,
                ft_lines,
                ed_cursor,
                true,
                editor_start_row + ed_cursor.row,
            )
        };

    let footer = footer_lines.join("\n");

    InteractiveLayout {
        lines,
        cursor,
        cursor_visible,
        cursor_row,
        queued_lines,
        widget_lines,
        working_line,
        top_divider,
        editor_lines,
        bottom_divider,
        footer_lines,
        footer,
    }
}
