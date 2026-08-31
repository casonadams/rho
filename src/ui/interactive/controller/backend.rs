use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    queue,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use std::io::{self, Stdout, Write};

pub trait TerminalBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()>;
    fn size(&self) -> io::Result<(u16, u16)>;
    fn hide_cursor(&mut self) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
    fn move_up(&mut self, rows: usize) -> io::Result<()>;
    fn move_down(&mut self, rows: usize) -> io::Result<()>;
    fn move_to_column(&mut self, column: usize) -> io::Result<()>;
    fn clear_line(&mut self) -> io::Result<()>;
    fn write_text(&mut self, text: &str) -> io::Result<()>;
    fn flush(&mut self) -> io::Result<()>;
}

pub struct CrosstermBackend {
    stdout: Stdout,
}

impl CrosstermBackend {
    pub fn stdout() -> Self {
        Self { stdout: io::stdout() }
    }
}

impl TerminalBackend for CrosstermBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        if enabled { enable_raw_mode() } else { disable_raw_mode() }
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        size()
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout, Hide)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        queue!(self.stdout, Show)
    }

    fn move_up(&mut self, rows: usize) -> io::Result<()> {
        queue_vertical_move(&mut self.stdout, rows, true)
    }

    fn move_down(&mut self, rows: usize) -> io::Result<()> {
        queue_vertical_move(&mut self.stdout, rows, false)
    }

    fn move_to_column(&mut self, column: usize) -> io::Result<()> {
        queue!(self.stdout, MoveToColumn(column.min(u16::MAX as usize) as u16))
    }

    fn clear_line(&mut self) -> io::Result<()> {
        queue!(self.stdout, Clear(ClearType::CurrentLine))
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        self.stdout.write_all(text.as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

pub(crate) fn queue_vertical_move(stdout: &mut Stdout, mut rows: usize, upward: bool) -> io::Result<()> {
    while rows > 0 {
        let step = rows.min(u16::MAX as usize) as u16;
        if upward {
            queue!(stdout, MoveUp(step))?;
        } else {
            queue!(stdout, MoveDown(step))?;
        }
        rows -= usize::from(step);
    }
    Ok(())
}
