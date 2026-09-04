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
    pub fn state(&self) -> &crate::ui::interactive::InteractiveState {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut crate::ui::interactive::InteractiveState {
        &mut self.state
    }

    pub fn terminal_width(&self) -> usize {
        self.width.max(1)
    }

    pub fn terminal_height(&self) -> usize {
        self.height.max(1)
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        paint::erase_live_region(&mut self.backend, self.rendered.as_ref())?;
        self.rendered = None;
        self.output.clear();
        let _ = self.backend.write_text("\x1b]111\x1b\\\x1b]110\x1b\\");
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
        if !self.theme.is_ansi() {
            if let Some(bg) = &self.theme.terminal_bg {
                let _ = self.backend.write_text(&format!("\x1b]11;{bg}\x1b\\"));
            }
            if let Some(fg) = &self.theme.terminal_fg {
                let _ = self.backend.write_text(&format!("\x1b]10;{fg}\x1b\\"));
            }
        }
        self.active = true;
        self.redraw()
    }

    pub(super) fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = paint::erase_live_region(&mut self.backend, self.rendered.as_ref());
        self.rendered = None;
        let _ = self.backend.write_text("\x1b]111\x1b\\\x1b]110\x1b\\");
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
