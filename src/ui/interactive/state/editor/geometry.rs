use unicode_width::UnicodeWidthChar;

pub(super) fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
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

pub(super) fn editor_cursor_position(text: &str, cursor: usize, terminal_width: usize) -> (usize, usize) {
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
