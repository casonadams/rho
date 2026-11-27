use std::io;

use super::backend::TerminalBackend;
use super::sgr::SgrStyle;
use super::stream::{Cursor, Token, tokens};

pub const CSI_BEGIN_SYNC_UPDATE: &str = "\x1b[?2026h";
pub const CSI_END_SYNC_UPDATE: &str = "\x1b[?2026l";
pub const SGR_RESET: &str = "\x1b[0m";
pub const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H\x1b[0m";
pub const CLEAR_SCREEN_AND_SCROLLBACK: &str = "\x1b[H\x1b[2J\x1b[3J\x1b[0m";
pub const CURSOR_HOME: &str = "\x1b[H";

/// Where the cursor sits after `value` is printed from the start of a row: its column, and
/// whether a full row is waiting to wrap.
pub fn output_cursor(value: &str, terminal_width: usize) -> (usize, bool) {
    let mut cursor = Cursor::new(terminal_width);
    for token in tokens(value) {
        match token {
            Token::Char('\r' | '\n') => cursor.return_to_line_start(),
            Token::Char(character) => cursor.advance(character),
            Token::Escape { .. } => {}
        }
    }
    (cursor.column(), cursor.wrap_pending())
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
    style: SgrStyle,
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
        self.style.clear();
    }

    /// The text style the terminal holds after the output so far. Repainting the live region between
    /// writes resets it, so the next write must start by restoring it.
    pub fn style(&self) -> &str {
        self.style.as_str()
    }

    fn track_style(&mut self, output: &str) {
        for token in tokens(output) {
            if let Token::Escape {
                sgr: Some(parameters), ..
            } = token
            {
                self.style.apply(parameters);
            }
        }
    }

    pub fn update(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        self.track_style(output);
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

    /// Whether the open line exactly fills its last row, so the restored cursor already sits on the next row.
    pub fn ends_at_wrap_boundary(&self, width: usize) -> bool {
        self.open && output_cursor(&self.line, width).1
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
