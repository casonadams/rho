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
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Held across every raw-mode transition so an abort from a signal handler or panic hook never
/// lands between enabling raw mode and recording it, which would leave the terminal raw.
static RAW_TERMINAL_ACTIVE: Mutex<bool> = Mutex::new(false);

fn lock_raw(active: &Mutex<bool>) -> MutexGuard<'_, bool> {
    active.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts the terminal back into cooked mode from a signal or panic path that cannot reach the
/// controller. Does nothing unless a crossterm backend currently holds the terminal in raw mode.
pub fn restore_terminal_after_abort() {
    abort_raw_session(&RAW_TERMINAL_ACTIVE, &mut io::stdout(), || {
        let _ = disable_raw_mode();
    });
}

/// Writes the escape sequences that undo [`write_setup_modes`] plus a visible cursor and runs
/// `disable`, once, and only while a raw session is active. Returns whether it did.
fn abort_raw_session(active: &Mutex<bool>, out: &mut impl Write, disable: impl FnOnce()) -> bool {
    let mut raw = lock_raw(active);
    if !std::mem::take(&mut *raw) {
        return false;
    }
    let _ = write_restore_modes(out);
    super::title::pop_title_on_abort(out);
    let _ = execute!(out, Show);
    disable();
    true
}

fn enter_raw_session(
    active: &Mutex<bool>,
    enable: impl FnOnce() -> io::Result<()>,
    setup: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut raw = lock_raw(active);
    enable()?;
    *raw = true;
    setup()
}

fn leave_raw_session(
    active: &Mutex<bool>,
    restore: impl FnOnce(),
    disable: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let mut raw = lock_raw(active);
    restore();
    *raw = false;
    disable()
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
            enter_raw_session(&RAW_TERMINAL_ACTIVE, enable_raw_mode, || {
                write_setup_modes(&mut self.stdout)
            })
        } else {
            leave_raw_session(
                &RAW_TERMINAL_ACTIVE,
                || {
                    let _ = write_restore_modes(&mut self.stdout);
                },
                disable_raw_mode,
            )
        }
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
        let raw = Mutex::new(true);
        let mut out = Vec::new();
        let mut disabled = 0;

        assert!(abort_raw_session(&raw, &mut out, || disabled += 1));
        let first = String::from_utf8(out.clone()).unwrap();
        assert!(
            first.starts_with(&written(|o| write_restore_modes(o).unwrap())),
            "{first:?}"
        );
        assert!(first.ends_with("\x1b[?25h"), "{first:?}");
        assert!(!*lock_raw(&raw));

        assert!(!abort_raw_session(&raw, &mut out, || disabled += 1));
        assert_eq!(String::from_utf8(out).unwrap(), first);
        assert_eq!(disabled, 1);
    }

    #[test]
    fn aborting_without_raw_mode_writes_nothing() {
        let raw = Mutex::new(false);
        let mut out = Vec::new();

        assert!(!abort_raw_session(&raw, &mut out, || panic!("must not disable")));

        assert!(out.is_empty());
    }

    #[test]
    fn an_abort_during_raw_mode_entry_waits_and_then_restores_the_terminal() {
        let raw = Mutex::new(false);
        let events = Mutex::new(Vec::new());
        let log = |event: &'static str| events.lock().unwrap().push(event);

        std::thread::scope(|scope| {
            let aborter = scope.spawn(|| {
                while !events.lock().unwrap().contains(&"enabled") {
                    std::thread::yield_now();
                }
                assert!(abort_raw_session(&raw, &mut Vec::new(), || log("disabled")));
            });
            enter_raw_session(
                &raw,
                || {
                    log("enabled");
                    Ok(())
                },
                || {
                    log("setup");
                    Ok(())
                },
            )
            .unwrap();
            aborter.join().unwrap();
        });

        assert_eq!(*events.lock().unwrap(), ["enabled", "setup", "disabled"]);
    }

    #[test]
    fn leaving_raw_mode_clears_the_flag_before_the_terminal_is_released() {
        let raw = Mutex::new(true);
        let mut flag_seen_by_disable = None;

        leave_raw_session(
            &raw,
            || {},
            || {
                flag_seen_by_disable = Some(raw.try_lock().is_err());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(flag_seen_by_disable, Some(true), "the gate is held while disabling");
        assert!(!*lock_raw(&raw));
    }
}
