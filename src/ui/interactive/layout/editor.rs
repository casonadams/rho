use crate::ui::interactive::state::editor::cursor::{cursor_position_in_lines, wrap_visual_lines};
use crate::ui::interactive::{CursorPosition, EditorState};
use unicode_width::UnicodeWidthChar;

pub(crate) fn wrap_editor(editor: &EditorState, width: usize) -> (Vec<String>, CursorPosition) {
    let max_width = width.max(1);
    let text = editor.text();
    let visual = wrap_visual_lines(text, max_width);
    let (row, column) = cursor_position_in_lines(text, &visual, editor.cursor(), max_width);
    let mut lines: Vec<String> = visual
        .iter()
        .map(|line| text[line.start..line.end].to_string())
        .collect();
    if row == lines.len() {
        lines.push(String::new());
    }
    (lines, CursorPosition { row, column })
}

pub(crate) fn wrap_editor_indented(editor: &EditorState, width: usize) -> (Vec<String>, CursorPosition) {
    let (lines, cursor) = wrap_editor(editor, width.saturating_sub(1).max(1));
    let indented = lines.into_iter().map(|line| format!(" {line}")).collect();
    let cursor = CursorPosition {
        row: cursor.row,
        column: cursor.column + 1,
    };
    (indented, cursor)
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

pub(crate) fn apply_software_cursor(line: &mut String, target_column: usize, ghost_text: Option<&str>) {
    let mut current_col = 0;
    let mut byte_offset = None;
    let mut char_len = 0;

    for (idx, ch) in line.char_indices() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_col == target_column || (cw > 1 && target_column > current_col && target_column < current_col + cw) {
            byte_offset = Some(idx);
            char_len = ch.len_utf8();
            break;
        }
        current_col += cw;
    }

    if let Some(offset) = byte_offset {
        let before = &line[..offset];
        let ch_str = &line[offset..offset + char_len];
        let after = &line[offset + char_len..];
        *line = format!("{before}\x1b[7m{ch_str}\x1b[27m{after}");
    } else {
        line.push_str("\x1b[7m \x1b[27m");
        if let Some(ghost) = ghost_text {
            line.push_str("\x1b[2m");
            line.push_str(ghost);
            line.push_str("\x1b[22m");
        }
    }
}

pub(crate) fn render_editor_lines(lines: Vec<String>, cursor: CursorPosition, ghost_text: Option<&str>) -> Vec<String> {
    let mut lines = lines;
    if cursor.row < lines.len() {
        apply_software_cursor(&mut lines[cursor.row], cursor.column, ghost_text);
    }
    lines
}
