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
    if write_abort_restore(&mut io::stdout(), &RAW_TERMINAL_ACTIVE) {
        let _ = disable_raw_mode();
    }
}

/// Writes the escape sequences that undo [`write_setup_modes`] plus a visible cursor, once, and only
/// while `raw_active` is set. Returns whether it wrote them.
fn write_abort_restore(out: &mut impl Write, raw_active: &AtomicBool) -> bool {
    if !raw_active.swap(false, Ordering::SeqCst) {
        return false;
    }
    let _ = write_restore_modes(out);
    let _ = execute!(out, Show);
    true
}

fn write_setup_modes(out: &mut impl Write) -> io::Result<()> {
    execute!(
        out,
        EnableBracketedPaste,
        EnableFocusChange,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
    )
}

fn write_restore_modes(out: &mut impl Write) -> io::Result<()> {
    execute!(
        out,
        PopKeyboardEnhancementFlags,
        DisableFocusChange,
        DisableBracketedPaste,
    )
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
            write_setup_modes(&mut self.stdout)?;
        } else {
            let _ = write_restore_modes(&mut self.stdout);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn written(write: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut out = Vec::new();
        write(&mut out);
        String::from_utf8(out).unwrap()
    }

    fn escapes(text: &str) -> Vec<&str> {
        text.split('\x1b').skip(1).collect()
    }

    #[test]
    fn every_mode_enabled_at_setup_is_disabled_by_the_restore() {
        let setup = written(|out| write_setup_modes(out).unwrap());
        let restore = written(|out| write_restore_modes(out).unwrap());

        let enabled: Vec<&str> = escapes(&setup);
        assert_eq!(enabled.len(), 3, "{setup:?}");
        for sequence in enabled {
            let undo = if let Some(mode) = sequence.strip_suffix('h') {
                format!("{mode}l")
            } else {
                assert!(sequence.starts_with("[>") && sequence.ends_with('u'), "{sequence:?}");
                "[<1u".to_string()
            };
            assert!(
                escapes(&restore).contains(&undo.as_str()),
                "{sequence:?} not undone: {restore:?}"
            );
        }
        assert_eq!(escapes(&restore).len(), 3, "{restore:?}");
    }

    #[test]
    fn an_aborted_raw_session_restores_modes_and_cursor_exactly_once() {
        let raw = AtomicBool::new(true);
        let mut out = Vec::new();

        assert!(write_abort_restore(&mut out, &raw));
        let first = String::from_utf8(out.clone()).unwrap();
        assert!(
            first.starts_with(&written(|o| write_restore_modes(o).unwrap())),
            "{first:?}"
        );
        assert!(first.ends_with("\x1b[?25h"), "{first:?}");
        assert!(!raw.load(Ordering::SeqCst));

        assert!(!write_abort_restore(&mut out, &raw));
        assert_eq!(String::from_utf8(out).unwrap(), first);
    }

    #[test]
    fn aborting_without_raw_mode_writes_nothing() {
        let raw = AtomicBool::new(false);
        let mut out = Vec::new();

        assert!(!write_abort_restore(&mut out, &raw));

        assert!(out.is_empty());
    }
}
