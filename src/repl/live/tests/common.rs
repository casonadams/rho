use crate::ui::interactive::TerminalBackend;
use std::io;

pub(super) struct HistoryTerminal;

impl TerminalBackend for HistoryTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((20, 24))
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

pub(super) struct RedrawCountingTerminal {
    pub(super) redraws: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl TerminalBackend for RedrawCountingTerminal {
    fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
        Ok(())
    }

    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((80, 24))
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.redraws.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

#[test]
fn live_ui_requires_both_terminal_streams() {
    use super::super::live_ui_supported;
    assert!(live_ui_supported(true, true));
    assert!(!live_ui_supported(true, false));
    assert!(!live_ui_supported(false, true));
    assert!(!live_ui_supported(false, false));
}
