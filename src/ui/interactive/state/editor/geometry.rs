use unicode_width::UnicodeWidthChar;

pub(super) fn editor_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .map(|(index, character)| index + character.len_utf8()),
    )
}

pub(super) fn editor_cursor_position(text: &str, cursor: usize, terminal_width: usize) -> (usize, usize) {
    let mut row = 0;
    let mut column = 0;
    for (byte_index, character) in text.char_indices() {
        if character == '\n' {
            if byte_index == cursor {
                return (row, column);
            }
            row += 1;
            column = 0;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > terminal_width {
            row += 1;
            column = 0;
        }
        if byte_index == cursor {
            return (row, column);
        }
        column += character_width;
    }
    if column == terminal_width {
        row += 1;
        column = 0;
    }
    (row, column)
}
