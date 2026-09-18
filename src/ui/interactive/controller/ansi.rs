use std::io;
use unicode_width::UnicodeWidthChar;

use super::backend::TerminalBackend;

pub const CSI_BEGIN_SYNC_UPDATE: &str = "\x1b[?2026h";
pub const CSI_END_SYNC_UPDATE: &str = "\x1b[?2026l";

fn skip_escape_sequence(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    if characters.next_if_eq(&'[').is_some() {
        skip_until_final_byte(characters, |c| ('@'..='~').contains(&c));
    } else if characters.next_if_eq(&']').is_some() {
        skip_until_final_byte(characters, |c| c == '\x07' || c == '\u{1b}');
    }
}

fn skip_until_final_byte(characters: &mut std::iter::Peekable<std::str::Chars<'_>>, is_final: impl Fn(char) -> bool) {
    for sequence_character in characters.by_ref() {
        if is_final(sequence_character) {
            break;
        }
    }
}

struct CursorState {
    column: usize,
    at_wrap_boundary: bool,
}

impl CursorState {
    fn advance(&mut self, character: char, terminal_width: usize) {
        let character_width = character.width().unwrap_or(0);
        if self.column > 0 && self.column + character_width > terminal_width {
            self.column = character_width;
            self.at_wrap_boundary = false;
            return;
        }
        self.column += character_width;
        self.at_wrap_boundary = self.column == terminal_width;
        if self.at_wrap_boundary {
            self.column = 0;
        }
    }
}

pub fn output_cursor(value: &str, terminal_width: usize) -> (usize, bool) {
    let mut state = CursorState {
        column: 0,
        at_wrap_boundary: false,
    };
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            skip_escape_sequence(&mut characters);
        } else if character == '\r' {
            state.column = 0;
            state.at_wrap_boundary = false;
        } else {
            state.advance(character, terminal_width.max(1));
        }
    }
    (state.column, state.at_wrap_boundary)
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

#[derive(Debug, Default)]
pub struct OutputTracker {
    line: String,
    open: bool,
}

impl OutputTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn clear(&mut self) {
        self.line.clear();
        self.open = false;
    }

    pub fn update(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(newline) = output.rfind('\n') {
            self.line.clear();
            self.line.push_str(&output[newline + 1..]);
        } else {
            self.line.push_str(output);
        }
        let has_newline =
            output.ends_with('\n') || (output.rfind('\n').is_some() && output_cursor(&self.line, usize::MAX).0 == 0);
        self.open = !has_newline;
        if !self.open {
            self.line.clear();
        }
    }

    pub fn restore_cursor<B: TerminalBackend>(&self, backend: &mut B, width: usize) -> io::Result<()> {
        if !self.open {
            return Ok(());
        }
        let (column, at_wrap_boundary) = output_cursor(&self.line, width);
        if !at_wrap_boundary {
            backend.move_up(1)?;
        }
        backend.move_to_column(column)
    }
}
