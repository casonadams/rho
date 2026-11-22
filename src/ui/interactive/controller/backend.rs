use crossterm::{
    cursor::{Hide, MoveDown, MoveToColumn, MoveUp, Show},
    event::{
        DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute, queue,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size},
};
use std::io::{self, BufWriter, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static RAW_TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Puts the terminal back into cooked mode from a signal or panic path that cannot reach the
/// controller. Does nothing unless a crossterm backend currently holds the terminal in raw mode.
pub fn restore_terminal_after_abort() {
    if RAW_TERMINAL_ACTIVE.swap(false, Ordering::SeqCst) {
        let mut out = io::stdout();
        let _ = execute!(
            out,
            PopKeyboardEnhancementFlags,
            DisableFocusChange,
            DisableBracketedPaste,
            Show
        );
        let _ = disable_raw_mode();
    }
}

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

#[cfg(test)]
pub(crate) struct NullBackend;

#[cfg(test)]
impl TerminalBackend for NullBackend {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn move_up(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_down(&mut self, _rows: usize) -> io::Result<()> {
        Ok(())
    }
    fn move_to_column(&mut self, _column: usize) -> io::Result<()> {
        Ok(())
    }
    fn clear_line(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_text(&mut self, _text: &str) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct CrosstermBackend {
    stdout: BufWriter<Stdout>,
}

impl CrosstermBackend {
    pub fn stdout() -> Self {
        // io::stdout() is line-buffered: unbuffered, every \r\n in a queued
        // frame flushes separately and the tty repaints mid-frame (flicker).
        Self {
            stdout: BufWriter::with_capacity(256 * 1024, io::stdout()),
        }
    }
}

impl TerminalBackend for CrosstermBackend {
    fn set_raw_mode(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            enable_raw_mode()?;
            RAW_TERMINAL_ACTIVE.store(true, Ordering::SeqCst);
            execute!(
                self.stdout,
                EnableBracketedPaste,
                EnableFocusChange,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
            )?;
        } else {
            let _ = execute!(
                self.stdout,
                PopKeyboardEnhancementFlags,
                DisableFocusChange,
                DisableBracketedPaste,
            );
            RAW_TERMINAL_ACTIVE.store(false, Ordering::SeqCst);
            disable_raw_mode()?;
        }
        Ok(())
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

pub(crate) fn queue_vertical_move(stdout: &mut impl Write, mut rows: usize, upward: bool) -> io::Result<()> {
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
