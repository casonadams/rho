use crate::ui::interactive::layout::editor::window_editor;
use crate::ui::interactive::layout::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

#[test]
fn window_editor_centers_cursor_in_middle() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor = CursorPosition { row: 10, column: 0 };
    let (windowed, new_cursor) = window_editor(lines, cursor, 5);
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
    let (windowed, new_cursor) = window_editor(lines, cursor_top, 5);
    let sample = (windowed.len(), windowed[0].as_str(), windowed[4].as_str(), new_cursor);
    assert_eq!(sample, (5, "line 0", "line 4", CursorPosition { row: 1, column: 0 }));
}

#[test]
fn window_editor_clamps_at_bottom() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor_bottom = CursorPosition { row: 19, column: 0 };
    let (windowed, new_cursor) = window_editor(lines, cursor_bottom, 5);
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
