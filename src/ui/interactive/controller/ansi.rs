use unicode_width::UnicodeWidthChar;

pub fn output_cursor(value: &str, terminal_width: usize) -> (usize, bool) {
    let terminal_width = terminal_width.max(1);
    let mut column = 0;
    let mut at_wrap_boundary = false;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.next_if_eq(&'[').is_some() {
                for sequence_character in characters.by_ref() {
                    if ('@'..='~').contains(&sequence_character) {
                        break;
                    }
                }
            } else if characters.next_if_eq(&']').is_some() {
                for sequence_character in characters.by_ref() {
                    if sequence_character == '\x07' || sequence_character == '\u{1b}' {
                        break;
                    }
                }
            }
            continue;
        }
        if character == '\r' {
            column = 0;
            at_wrap_boundary = false;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if column > 0 && column + character_width > terminal_width {
            column = 0;
        }
        column += character_width;
        at_wrap_boundary = column == terminal_width;
        if at_wrap_boundary {
            column = 0;
        }
    }
    (column, at_wrap_boundary)
}

pub fn terminal_newlines(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut previous_was_carriage_return = false;
    for character in value.chars() {
        if character == '\n' && !previous_was_carriage_return {
            result.push('\r');
        }
        result.push(character);
        previous_was_carriage_return = character == '\r';
    }
    result
}
