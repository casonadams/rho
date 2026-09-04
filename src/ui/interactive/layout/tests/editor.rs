use crate::ui::interactive::layout::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

#[test]
fn empty_editor_has_one_line_and_fixed_chrome() {
    let default_editor = EditorState::default();
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &default_editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 8,
        spinner_frame: 0,
    });

    assert_eq!(layout.top_divider, "\u{1b}[2m────────\u{1b}[0m");
    assert_eq!(layout.editor_lines, [""]);
    assert_eq!(layout.footer_lines.len(), 2);
    assert_eq!(layout.cursor, CursorPosition { row: 0, column: 0 });
    assert_eq!(layout.height(), 6);
}

#[test]
fn explicit_newlines_grow_the_editor() {
    let mut editor = EditorState::default();
    editor.set_text("one\ntwo\n");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 20,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["one", "two", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
    assert_eq!(layout.height(), 8);
}

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
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 4,
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
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 3,
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
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 2,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines, ["界", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 0 });
}

#[test]
fn editor_layout_tracks_lines_and_dividers() {
    let mut editor = EditorState::default();
    editor.set_text("draft");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 1);
    assert_eq!(layout.height(), 6);
    assert_eq!(layout.cursor_row(), 2);
}

#[test]
fn multiline_editor_height_matches_content() {
    let mut editor = EditorState::default();
    editor.set_text("line1\nline2\nline3");
    let default_footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &default_footer,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        spinner_frame: 0,
    });

    assert_eq!(layout.editor_lines.len(), 3);
    assert_eq!(layout.height(), 8);
}
