//! Editor layout tests: sizing, windowing, and soft-wrap cursor tracking.

use crate::ui::interactive::layout::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

fn empty_editor_layout(width: usize) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor: &EditorState::default(),
        modal: None,
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: width,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    })
}

#[test]
fn empty_editor_has_one_line_and_fixed_chrome() {
    let layout = empty_editor_layout(8);
    assert_eq!(layout.editor_lines, [""]);
    let actual = (
        layout.top_divider.as_str(),
        layout.footer_lines.len(),
        layout.cursor,
        layout.height(),
    );
    assert_eq!(
        actual,
        ("\u{1b}[2m────────\u{1b}[0m", 2, CursorPosition { row: 0, column: 0 }, 7)
    );
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 20,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(layout.editor_lines, ["one", "two", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
    assert_eq!(layout.height(), 9);
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(layout.editor_lines.len(), 1);
    assert_eq!(layout.height(), 7);
    assert_eq!(layout.cursor_row(), 3);
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
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 24,
        spinner_frame: 0,
        theme: None,
    });

    assert_eq!(layout.editor_lines.len(), 3);
    assert_eq!(layout.height(), 9);
}

// ---------------------------------------------------------------------------
// Windowing
// ---------------------------------------------------------------------------

#[test]
fn window_editor_centers_cursor_in_middle() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor = CursorPosition { row: 10, column: 0 };
    let (windowed, new_cursor) = crate::ui::interactive::layout::editor::window_editor(lines, cursor, 5);
    let sample = (
        windowed.len(),
        windowed[0].as_str(),
        windowed[2].as_str(),
        windowed[4].as_str(),
        new_cursor,
    );
    assert_eq!(
        sample,
        (5, "line 8", "line 10", "line 12", CursorPosition { row: 2, column: 0 })
    );
}

#[test]
fn window_editor_clamps_at_top() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor_top = CursorPosition { row: 1, column: 0 };
    let (windowed, new_cursor) = crate::ui::interactive::layout::editor::window_editor(lines, cursor_top, 5);
    let sample = (windowed.len(), windowed[0].as_str(), windowed[4].as_str(), new_cursor);
    assert_eq!(sample, (5, "line 0", "line 4", CursorPosition { row: 1, column: 0 }));
}

#[test]
fn window_editor_clamps_at_bottom() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor_bottom = CursorPosition { row: 19, column: 0 };
    let (windowed, new_cursor) = crate::ui::interactive::layout::editor::window_editor(lines, cursor_bottom, 5);
    let sample = (windowed.len(), windowed[0].as_str(), windowed[4].as_str(), new_cursor);
    assert_eq!(sample, (5, "line 15", "line 19", CursorPosition { row: 4, column: 0 }));
}

fn test_window_layout(
    editor: &EditorState,
    terminal_height: usize,
) -> crate::ui::interactive::layout::InteractiveLayout {
    layout(LayoutInput {
        editor,
        modal: None,
        autocomplete: None,
        footer: &FooterState::default(),
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height,
        spinner_frame: 0,
        theme: None,
    })
}

#[test]
fn multiline_editor_windowed_to_terminal_height_when_oversized() {
    let mut editor = EditorState::default();
    let text = (0..50).map(|i| format!("code line {i}")).collect::<Vec<_>>().join("\n");
    editor.set_text(&text);

    let layout = test_window_layout(&editor, 12);
    assert!(layout.height() <= 12 && layout.cursor_row() < layout.height());
    assert!(!layout.top_divider.is_empty() && !layout.bottom_divider.is_empty() && !layout.footer_lines.is_empty());
}

#[test]
fn multiline_editor_cursor_tracking_within_window() {
    let mut editor = EditorState::default();
    let text = (0..50).map(|i| format!("line_{i}")).collect::<Vec<_>>().join("\n");
    editor.set_text(&text);
    editor.move_to_start();
    for _ in 0..25 {
        editor.move_down(80);
    }

    let layout = test_window_layout(&editor, 15);
    assert!(layout.height() <= 15 && layout.cursor_row() < layout.height());
    assert!(layout.lines[layout.cursor_row()].contains("line_25"));
}

fn assert_minimal_height_layout(l: &crate::ui::interactive::layout::InteractiveLayout, h: usize) {
    assert!(l.height() <= h.max(1));
    assert!(l.cursor_row() < l.height());
}

#[test]
fn minimal_terminal_height_graceful_degradation() {
    let mut editor = EditorState::default();
    editor.set_text("test draft");

    for h in 0..=6 {
        let l = layout(LayoutInput {
            editor: &editor,
            modal: None,
            autocomplete: None,
            footer: &FooterState::default(),
            system_message: None,
            queued_messages: &[],
            widget_lines: &[],
            terminal_width: 40,
            terminal_height: h,
            spinner_frame: 0,
            theme: None,
        });
        assert_minimal_height_layout(&l, h);
    }
}

// ---------------------------------------------------------------------------
// Soft wrap
// ---------------------------------------------------------------------------

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
        theme: None,
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
        theme: None,
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
        theme: None,
    });

    assert_eq!(layout.editor_lines, ["界", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 1, column: 0 });
}
