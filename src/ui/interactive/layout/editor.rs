use crate::ui::interactive::{CursorPosition, EditorState};
use unicode_width::UnicodeWidthChar;

struct EditorWrapper {
    lines: Vec<String>,
    row: usize,
    column: usize,
    cursor: Option<CursorPosition>,
    target_cursor: usize,
    width: usize,
}

impl EditorWrapper {
    fn new(target_cursor: usize, width: usize) -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            column: 0,
            cursor: None,
            target_cursor,
            width,
        }
    }

    fn push_newline(&mut self, byte_index: usize) {
        if byte_index == self.target_cursor {
            self.cursor = Some(CursorPosition {
                row: self.row,
                column: self.column,
            });
        }
        self.lines.push(String::new());
        self.row += 1;
        self.column = 0;
    }

    fn push_char(&mut self, byte_index: usize, c: char) {
        let cw = c.width().unwrap_or(0);
        if self.column > 0 && self.column + cw > self.width {
            self.lines.push(String::new());
            self.row += 1;
            self.column = 0;
        }
        if byte_index == self.target_cursor {
            self.cursor = Some(CursorPosition {
                row: self.row,
                column: self.column,
            });
        }
        self.lines[self.row].push(c);
        self.column += cw;
    }

    fn finish(mut self, text_len: usize) -> (Vec<String>, CursorPosition) {
        if self.target_cursor == text_len {
            if self.column == self.width {
                self.lines.push(String::new());
                self.row += 1;
                self.column = 0;
            }
            self.cursor = Some(CursorPosition {
                row: self.row,
                column: self.column,
            });
        }
        (
            self.lines,
            self.cursor
                .expect("editor cursor must be on a UTF-8 character boundary"),
        )
    }
}

pub(crate) fn wrap_editor(editor: &EditorState, width: usize) -> (Vec<String>, CursorPosition) {
    let mut wrapper = EditorWrapper::new(editor.cursor(), width);
    for (idx, c) in editor.text().char_indices() {
        if c == '\n' {
            wrapper.push_newline(idx);
        } else {
            wrapper.push_char(idx, c);
        }
    }
    wrapper.finish(editor.text().len())
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

pub(crate) fn apply_software_cursor(line: &mut String, target_column: usize) {
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
    }
}

pub(crate) fn render_editor_lines(lines: Vec<String>, cursor: CursorPosition) -> Vec<String> {
    let mut lines = lines;
    if cursor.row < lines.len() {
        apply_software_cursor(&mut lines[cursor.row], cursor.column);
    }
    lines
}
