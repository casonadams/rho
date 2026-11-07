//! Editor layout tests: sizing, windowing, and soft-wrap cursor tracking.

use super::base_input;
use crate::ui::interactive::layout::{CursorPosition, InteractiveLayout, LayoutInput, layout};
use crate::ui::interactive::{EditorState, FooterState};
use crate::ui::theme::Theme;

fn editor_layout(editor: &EditorState, width: usize, height: usize) -> InteractiveLayout {
    layout(LayoutInput {
        terminal_width: width,
        terminal_height: height,
        ..base_input(editor, &FooterState::default())
    })
}

fn editor_with_text(text: &str) -> EditorState {
    let mut editor = EditorState::default();
    editor.set_text(text);
    editor
}

fn fifty_line_editor(prefix: &str) -> EditorState {
    editor_with_text(&(0..50).map(|i| format!("{prefix}{i}")).collect::<Vec<_>>().join("\n"))
}

fn layouts_by_focus(editor: &EditorState, theme: &Theme) -> (InteractiveLayout, InteractiveLayout) {
    let footer = FooterState::default();
    let input = LayoutInput {
        theme: Some(theme),
        ..base_input(editor, &footer)
    };
    (
        layout(input),
        layout(LayoutInput {
            focused: false,
            ..input
        }),
    )
}

#[test]
fn empty_editor_has_one_line_and_fixed_chrome() {
    let layout = editor_layout(&EditorState::default(), 8, 24);
    assert_eq!(layout.editor_lines, [""]);
    let actual = (
        layout.top_divider.as_str(),
        layout.footer_lines.len(),
        layout.cursor,
        layout.height(),
    );
    assert_eq!(
        actual,
        ("\u{1b}[2m────────\u{1b}[0m", 2, CursorPosition { row: 0, column: 0 }, 6)
    );
}

#[test]
fn explicit_newlines_grow_the_editor() {
    let editor = editor_with_text("one\ntwo\n");
    let layout = editor_layout(&editor, 20, 24);

    assert_eq!(layout.editor_lines, ["one", "two", ""]);
    assert_eq!(layout.cursor, CursorPosition { row: 2, column: 0 });
    assert_eq!(layout.height(), 8);
}

#[test]
fn editor_height_tracks_content_lines() {
    for (text, line_count, height) in [("draft", 1, 6), ("line1\nline2\nline3", 3, 8)] {
        let layout = editor_layout(&editor_with_text(text), 80, 24);

        assert_eq!(layout.editor_lines.len(), line_count);
        assert_eq!(layout.height(), height);
    }
    assert_eq!(editor_layout(&editor_with_text("draft"), 80, 24).cursor_row(), 2);
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

#[test]
fn multiline_editor_windowed_to_terminal_height_when_oversized() {
    let editor = fifty_line_editor("code line ");

    let layout = editor_layout(&editor, 80, 12);
    assert!(layout.height() <= 12 && layout.cursor_row() < layout.height());
    assert!(!layout.top_divider.is_empty() && !layout.bottom_divider.is_empty() && !layout.footer_lines.is_empty());
}

#[test]
fn multiline_editor_cursor_tracking_within_window() {
    let mut editor = fifty_line_editor("line_");
    editor.move_to_start();
    for _ in 0..25 {
        editor.move_down(80);
    }

    let layout = editor_layout(&editor, 80, 15);
    assert!(layout.height() <= 15 && layout.cursor_row() < layout.height());
    let stripped = crate::ui::block::strip_ansi(&layout.lines[layout.cursor_row()]);
    assert!(stripped.contains("line_25"));
}

fn assert_minimal_height_layout(l: &InteractiveLayout, h: usize) {
    assert!(l.height() <= h.max(1));
    assert!(l.cursor_row() < l.height());
}

#[test]
fn minimal_terminal_height_graceful_degradation() {
    let editor = editor_with_text("test draft");

    for h in 0..=6 {
        let l = editor_layout(&editor, 40, h);
        assert_minimal_height_layout(&l, h);
    }
}

// ---------------------------------------------------------------------------
// Soft wrap
// ---------------------------------------------------------------------------

#[test]
fn soft_wrap_tracks_lines_and_cursor_across_wrapped_lines() {
    let cases: [(&str, usize, usize, &[&str], CursorPosition); 3] = [
        ("ab界c", 0, 4, &["ab界", "c"], CursorPosition { row: 1, column: 1 }),
        ("abcdef", 2, 3, &["abc", "def"], CursorPosition { row: 1, column: 1 }),
        ("界", 0, 2, &["界", ""], CursorPosition { row: 1, column: 0 }),
    ];
    for (text, left_moves, width, lines, cursor) in cases {
        let mut editor = editor_with_text(text);
        for _ in 0..left_moves {
            editor.move_left();
        }
        let layout = editor_layout(&editor, width, 24);
        assert_eq!(layout.editor_lines, lines, "text {text:?}");
        assert_eq!(layout.cursor, cursor, "text {text:?}");
    }
}

#[test]
fn software_cursor_rendering_cases() {
    use crate::ui::interactive::layout::editor::apply_software_cursor;

    // Case 1: Empty line
    let mut line = String::new();
    apply_software_cursor(&mut line, 0, None);
    assert_eq!(line, "\x1b[7m \x1b[27m");

    // Case 2: End of line
    let mut line = "hello".to_string();
    apply_software_cursor(&mut line, 5, None);
    assert_eq!(line, "hello\x1b[7m \x1b[27m");

    // Case 3: Start of line
    let mut line = "hello".to_string();
    apply_software_cursor(&mut line, 0, None);
    assert_eq!(line, "\x1b[7mh\x1b[27mello");

    // Case 4: Mid-line character
    let mut line = "hello".to_string();
    apply_software_cursor(&mut line, 2, None);
    assert_eq!(line, "he\x1b[7ml\x1b[27mlo");

    // Case 5: Wide Unicode character
    let mut line = "a界b".to_string();
    apply_software_cursor(&mut line, 1, None);
    assert_eq!(line, "a\x1b[7m界\x1b[27mb");

    // Case 6: Ghost text at end of line
    let mut line = "hello".to_string();
    apply_software_cursor(&mut line, 5, Some(" world"));
    assert_eq!(line, "hello\x1b[7m \x1b[27m\x1b[2m world\x1b[22m");
}

#[test]
fn hardware_cursor_renders_ghost_text() {
    let mut editor = editor_with_text("hello");
    editor.set_ghost_text(Some(" world".to_string()));

    let layout = editor_layout(&editor, 80, 24);

    assert_eq!(layout.editor_lines, ["hello\x1b[2m world\x1b[22m"]);
}

#[test]
fn unfocused_editor_suppresses_software_cursor() {
    let editor = editor_with_text("hello");
    let software_theme = Theme {
        cursor_mode: crate::ui::theme::CursorMode::Software,
        ..Default::default()
    };

    let (focused, unfocused) = layouts_by_focus(&editor, &software_theme);
    assert!(focused.cursor_visible);
    assert!(focused.editor_lines.iter().any(|l| l.contains("\x1b[7m")));

    assert!(!unfocused.cursor_visible);
    assert!(!unfocused.editor_lines.iter().any(|l| l.contains("\x1b[7m")));
    assert_eq!(unfocused.editor_lines, ["hello"]);
}

#[test]
fn hardware_cursor_mode_suppresses_software_cursor_and_preserves_hardware_visibility() {
    let editor = editor_with_text("hello");
    let theme = Theme {
        cursor_mode: crate::ui::theme::CursorMode::Hardware,
        ..Default::default()
    };

    let (focused, unfocused) = layouts_by_focus(&editor, &theme);
    assert!(focused.cursor_visible);
    assert_eq!(focused.cursor_mode, crate::ui::theme::CursorMode::Hardware);
    assert!(!focused.editor_lines.iter().any(|l| l.contains("\x1b[7m")));
    assert_eq!(focused.editor_lines, ["hello"]);

    assert!(!unfocused.cursor_visible);
    assert_eq!(unfocused.cursor_mode, crate::ui::theme::CursorMode::Hardware);
    assert!(!unfocused.editor_lines.iter().any(|l| l.contains("\x1b[7m")));
}
