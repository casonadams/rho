use crate::ui::interactive::layout::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

#[test]
fn soft_wrap_uses_display_width_for_wide_unicode() {
    let mut editor = EditorState::default();
    editor.set_text("ab界c");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 4,
        terminal_height: 24,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["ab界", "c"]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
}

#[test]
fn cursor_tracks_insertion_position_across_wrapped_lines() {
    let mut editor = EditorState::default();
    editor.set_text("abcdef");
    editor.move_left();
    editor.move_left();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 3,
        terminal_height: 24,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["abc", "def"]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 1 });
}

#[test]
fn full_final_line_adds_a_cursor_line() {
    let mut editor = EditorState::default();
    editor.set_text("界");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 2,
        terminal_height: 24,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["界", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 0 });
}
