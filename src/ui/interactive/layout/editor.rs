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
