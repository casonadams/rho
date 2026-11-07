use crate::ui::interactive::layout::LayoutInput;
use crate::ui::interactive::{EditorState, FooterState};

mod activity;
mod autocomplete;
mod budget;
mod divider;
mod editor;
mod modal;
mod running_tool;
mod status;
mod system_message;
mod text;
mod widget;

fn base_input<'a>(editor: &'a EditorState, footer: &'a FooterState) -> LayoutInput<'a> {
    LayoutInput {
        editor,
        modal: None,
        autocomplete: None,
        footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
        focused: true,
    }
}
