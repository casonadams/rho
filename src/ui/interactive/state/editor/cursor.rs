use unicode_width::UnicodeWidthChar;

use crate::ui::interactive::state::paste::find_marker_covering;

type CharIndicesItr<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

pub(crate) fn skip_whitespace_back(slice: &str) -> std::iter::Peekable<std::iter::Rev<std::str::CharIndices<'_>>> {
    let mut chars = slice.char_indices().rev().peekable();
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    chars
}

pub(crate) fn skip_whitespace_fwd(slice: &str) -> CharIndicesItr<'_> {
    let mut chars = slice.char_indices().peekable();
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    chars
}

pub(crate) fn char_class(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub(crate) fn snap_to_marker_end(text: &str, cursor: &mut usize, start: bool) {
    if let Some(marker) = find_marker_covering(text, *cursor) {
        *cursor = if start { marker.start } else { marker.end };
    }
}

pub(crate) fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    )
}

#[derive(Default)]
struct CursorWalk {
    row: usize,
    column: usize,
    cursor: usize,
    terminal_width: usize,
}

impl CursorWalk {
    fn new(cursor: usize, terminal_width: usize) -> Self {
        Self {
            cursor,
            terminal_width,
            ..Default::default()
        }
    }

    fn step(&mut self, byte_index: usize, character: char) -> Option<(usize, usize)> {
        if character == '\n' {
            if byte_index == self.cursor {
                return Some((self.row, self.column));
            }
            self.row += 1;
            self.column = 0;
            return None;
        }
        if wraps_to_next_row(self.column, character, self.terminal_width) {
            self.row += 1;
            self.column = 0;
        }
        if byte_index == self.cursor {
            return Some((self.row, self.column));
        }
        self.column += character.width().unwrap_or(0);
        None
    }
}

fn wraps_to_next_row(column: usize, character: char, terminal_width: usize) -> bool {
    let character_width = character.width().unwrap_or(0);
    column > 0 && column + character_width > terminal_width
}

pub(crate) fn editor_cursor_position(text: &str, cursor: usize, terminal_width: usize) -> (usize, usize) {
    let mut walk = CursorWalk::new(cursor, terminal_width.max(1));
    for (byte_index, character) in text.char_indices() {
        if let Some(position) = walk.step(byte_index, character) {
            return position;
        }
    }
    if walk.column == walk.terminal_width {
        walk.row += 1;
        walk.column = 0;
    }
    (walk.row, walk.column)
}

pub(crate) fn compute_vertical_target(
    text: &str,
    cursor: usize,
    terminal_width: usize,
    row_delta: isize,
    preferred_column: Option<usize>,
) -> Option<(usize, usize)> {
    let terminal_width = terminal_width.max(1);
    let (current_row, current_column) = editor_cursor_position(text, cursor, terminal_width);
    let target_row = current_row.checked_add_signed(row_delta)?;
    let pref_col = preferred_column.unwrap_or(current_column);
    let target = editor_boundaries(text)
        .map(|cur| {
            let (row, column) = editor_cursor_position(text, cur, terminal_width);
            (cur, row, column)
        })
        .filter(|(_, row, _)| *row == target_row)
        .min_by_key(|(_, _, column)| column.abs_diff(pref_col));

    let (mut new_cursor, _, _) = target?;
    if let Some(marker) = find_marker_covering(text, new_cursor) {
        let to_start = new_cursor - marker.start;
        let to_end = marker.end - new_cursor;
        new_cursor = if to_start <= to_end { marker.start } else { marker.end };
    }
    Some((new_cursor, pref_col))
}
