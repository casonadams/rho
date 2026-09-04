use crate::ui::interactive::layout::editor::window_editor;
use crate::ui::interactive::layout::{CursorPosition, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};

#[test]
fn window_editor_centers_cursor_in_middle() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    let cursor = CursorPosition { row: 10, column: 0 };
    let (windowed, new_cursor) = window_editor(lines, cursor, 5);

    assert_eq!(windowed.len(), 5);
    assert_eq!(windowed[0], "line 8");
    assert_eq!(windowed[2], "line 10");
    assert_eq!(windowed[4], "line 12");
    assert_eq!(new_cursor, CursorPosition { row: 2, column: 0 });
}

#[test]
fn window_editor_clamps_at_boundaries() {
    let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();

    let cursor_top = CursorPosition { row: 1, column: 0 };
    let (windowed_top, new_cursor_top) = window_editor(lines.clone(), cursor_top, 5);
    assert_eq!(windowed_top.len(), 5);
    assert_eq!(windowed_top[0], "line 0");
    assert_eq!(windowed_top[4], "line 4");
    assert_eq!(new_cursor_top, CursorPosition { row: 1, column: 0 });

    let cursor_bottom = CursorPosition { row: 19, column: 0 };
    let (windowed_bottom, new_cursor_bottom) = window_editor(lines, cursor_bottom, 5);
    assert_eq!(windowed_bottom.len(), 5);
    assert_eq!(windowed_bottom[0], "line 15");
    assert_eq!(windowed_bottom[4], "line 19");
    assert_eq!(new_cursor_bottom, CursorPosition { row: 4, column: 0 });
}

#[test]
fn multiline_editor_windowed_to_terminal_height_when_oversized() {
    let mut editor = EditorState::default();
    let text = (0..50).map(|i| format!("code line {i}")).collect::<Vec<_>>().join("\n");
    editor.set_text(&text);

    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 12,
        spinner_frame: 0,
    });

    assert!(layout.height() <= 12, "layout height {} must be <= 12", layout.height());
    assert!(layout.cursor_row() < layout.height());
    assert!(!layout.top_divider.is_empty());
    assert!(!layout.bottom_divider.is_empty());
    assert!(!layout.footer_lines.is_empty());
}

#[test]
fn multiline_editor_cursor_tracking_within_window() {
    let mut editor = EditorState::default();
    let text = (0..50).map(|i| format!("line_{i}")).collect::<Vec<_>>().join("\n");
    editor.set_text(&text);

    // Move cursor to line 25
    editor.move_to_start();
    for _ in 0..25 {
        editor.move_down(80);
    }

    let footer = FooterState::default();
    let layout = layout(LayoutInput {
        editor: &editor,
        modal: None,
        autocomplete: None,
        footer: &footer,
        system_message: None,
        queued_messages: &[],
        widget_lines: &[],
        terminal_width: 80,
        terminal_height: 15,
        spinner_frame: 0,
    });

    assert!(layout.height() <= 15);
    assert!(layout.cursor_row() < layout.height());
    assert!(
        layout.lines[layout.cursor_row()].contains("line_25"),
        "cursor row content: {}",
        layout.lines[layout.cursor_row()]
    );
}

#[test]
fn minimal_terminal_height_graceful_degradation() {
    let mut editor = EditorState::default();
    editor.set_text("test draft");
    let footer = FooterState::default();

    for h in 0..=6 {
        let l = layout(LayoutInput {
            editor: &editor,
            modal: None,
            autocomplete: None,
            footer: &footer,
            system_message: None,
            queued_messages: &[],
            widget_lines: &[],
            terminal_width: 40,
            terminal_height: h,
            spinner_frame: 0,
        });

        let expected_max = h.max(1);
        assert!(
            l.height() <= expected_max,
            "height {} > expected max {} for terminal_height {}",
            l.height(),
            expected_max,
            h
        );
        assert!(
            l.cursor_row() < l.height(),
            "cursor_row {} >= height {} for terminal_height {}",
            l.cursor_row(),
            l.height(),
            h
        );
    }
}
