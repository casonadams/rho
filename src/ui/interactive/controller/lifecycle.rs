use std::io;

use super::TerminalController;
use super::backend::{CrosstermBackend, TerminalBackend};
use super::paint;

impl TerminalController<CrosstermBackend> {
    pub fn stdout(state: crate::ui::interactive::InteractiveState) -> io::Result<Self> {
        Self::new(CrosstermBackend::stdout(), state)
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        paint::erase_live_region(&mut self.backend, self.rendered.as_ref())?;
        self.rendered = None;
        self.output.clear();
        self.backend.show_cursor()?;
        self.backend.set_raw_mode(false)?;
        self.backend.flush()?;
        self.active = false;
        Ok(())
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if self.active {
            return Ok(());
        }
        self.backend.set_raw_mode(true)?;
        self.active = true;
        self.redraw()
    }

    pub(super) fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = paint::erase_live_region(&mut self.backend, self.rendered.as_ref());
        self.rendered = None;
        let _ = self.backend.show_cursor();
        let _ = self.backend.set_raw_mode(false);
        let _ = self.backend.flush();
        self.active = false;
    }
}

impl<B: TerminalBackend> Drop for TerminalController<B> {
    fn drop(&mut self) {
        self.restore();
    }
}
