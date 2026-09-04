use crate::ui::interactive::{CursorPosition, EditorState};
use unicode_width::UnicodeWidthChar;

pub(crate) fn wrap_editor(editor: &EditorState, width: usize) -> (Vec<String>, CursorPosition) {
    let mut lines = vec![String::new()];
    let mut row = 0;
    let mut column = 0;
    let mut cursor = None;

    for (byte_index, character) in editor.text().char_indices() {
        if character == '\n' {
            if byte_index == editor.cursor() {
                cursor = Some(CursorPosition { row, column });
            }
            lines.push(String::new());
            row += 1;
            column = 0;
            continue;
        }

        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > width {
            lines.push(String::new());
            row += 1;
            column = 0;
        }
        if byte_index == editor.cursor() {
            cursor = Some(CursorPosition { row, column });
        }
        lines[row].push(character);
        column += character_width;
    }

    if editor.cursor() == editor.text().len() {
        if column == width {
            lines.push(String::new());
            row += 1;
            column = 0;
        }
        cursor = Some(CursorPosition { row, column });
    }

    (
        lines,
        cursor.expect("editor cursor must be on a UTF-8 character boundary"),
    )
}

pub(crate) fn window_editor(
    lines: Vec<String>,
    cursor: CursorPosition,
    max_lines: usize,
) -> (Vec<String>, CursorPosition) {
    let total = lines.len();
    let max_lines = max_lines.max(1);
    if total <= max_lines {
        return (lines, cursor);
    }

    let half = max_lines / 2;
    let ideal_start = cursor.row.saturating_sub(half);
    let start = ideal_start.min(total.saturating_sub(max_lines));
    let windowed = lines.into_iter().skip(start).take(max_lines).collect();
    let new_cursor = CursorPosition {
        row: cursor.row - start,
        column: cursor.column,
    };
    (windowed, new_cursor)
}
