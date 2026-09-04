use std::io;

use super::ansi::output_cursor;
use super::backend::TerminalBackend;

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
