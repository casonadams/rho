pub mod ansi;
pub mod backend;
pub mod cache;
pub mod paint;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod transcript;

pub use ansi::OutputTracker;
use ansi::terminal_newlines;
pub use backend::{CrosstermBackend, TerminalBackend};

use std::io;
use std::time::{Duration, Instant};

use super::{InteractiveLayout, InteractiveState, LayoutInput, layout};

pub const SYSTEM_MESSAGE_DURATION: Duration = Duration::from_secs(3);

pub struct TerminalController<B: TerminalBackend> {
    pub(super) backend: B,
    pub(super) state: InteractiveState,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) rendered: Option<InteractiveLayout>,
    pub(super) output: OutputTracker,
    pub(super) streamed_output: String,
    pub(super) spinner_frame: usize,
    pub(super) active: bool,
    pub(super) theme: crate::ui::theme::Theme,
    pub(super) transcript: Vec<super::TranscriptItem>,
    pub(super) cache: cache::TranscriptRenderCache,
    pub(super) system_message_expires_at: Option<std::time::Instant>,
    pub(super) focused: bool,
}

fn init_terminal<B: TerminalBackend>(backend: &mut B) -> io::Result<(usize, usize)> {
    backend.set_raw_mode(true)?;
    backend.hide_cursor()?;
    match backend.size() {
        Ok((w, h)) => Ok((usize::from(w), usize::from(h))),
        Err(err) => {
            let _ = backend.show_cursor();
            let _ = backend.set_raw_mode(false);
            Err(err)
        }
    }
}

impl TerminalController<CrosstermBackend> {
    pub fn stdout(state: crate::ui::interactive::InteractiveState) -> io::Result<Self> {
        Self::new(CrosstermBackend::stdout(), state)
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(mut backend: B, state: InteractiveState) -> io::Result<Self> {
        let (width, height) = init_terminal(&mut backend)?;
        let mut controller = Self {
            backend,
            state,
            width,
            height,
            rendered: None,
            output: OutputTracker::new(),
            streamed_output: String::new(),
            spinner_frame: 0,
            active: true,
            theme: crate::ui::theme::Theme::default(),
            transcript: Vec::new(),
            cache: cache::TranscriptRenderCache::new(),
            system_message_expires_at: None,
            focused: true,
        };
        if let Err(error) = controller.redraw() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        let rendered = self.current_layout();
        paint::render_live_diff(&mut self.backend, self.rendered.as_ref(), &rendered)?;
        self.rendered = Some(rendered);
        Ok(())
    }

    fn prepare_output_write(&mut self) -> io::Result<()> {
        self.backend.write_text(ansi::CSI_BEGIN_SYNC_UPDATE)?;
        self.backend.hide_cursor()?;
        paint::erase_live_region(&mut self.backend, self.rendered.as_ref())?;
        self.rendered = None;
        self.output.restore_cursor(&mut self.backend, self.width)
    }

    fn finish_output_write(&mut self) -> io::Result<()> {
        let rendered = self.current_layout();
        paint::write_live_region(&mut self.backend, &rendered)?;
        paint::apply_cursor_visibility(&mut self.backend, &rendered)?;
        self.rendered = Some(rendered);
        self.backend.write_text(ansi::CSI_END_SYNC_UPDATE)?;
        self.backend.flush()
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        let output = terminal_newlines(output);
        self.write_normalized_output(&output)
    }

    pub fn write_stream_output(&mut self, output: &str) -> io::Result<()> {
        let output = terminal_newlines(output);
        self.streamed_output.push_str(&output);
        self.write_normalized_output(&output)
    }

    pub fn commit_streamed_output(&mut self) {
        self.streamed_output.clear();
    }

    fn write_normalized_output(&mut self, output: &str) -> io::Result<()> {
        let prefix_newline = self.output.is_open() && output.trim_start_matches('\r').starts_with('╭');
        self.prepare_output_write()?;
        if prefix_newline {
            self.backend.write_text("\r\n")?;
            self.output.update("\n");
        }
        self.backend.write_text(output)?;
        self.output.update(output);
        if self.output.is_open() {
            self.backend.write_text("\r\n")?;
        }
        self.finish_output_write()
    }

    pub fn resize_to(&mut self, width: usize, height: usize) -> io::Result<bool> {
        let (actual_w, actual_h) = self
            .backend
            .size()
            .map(|(w, h)| (usize::from(w), usize::from(h)))
            .unwrap_or((width, height));
        let w = if actual_w != self.width { actual_w } else { width };
        let h = if actual_h != self.height { actual_h } else { height };
        self.apply_size(w, h)
    }

    pub fn refresh_size(&mut self) -> io::Result<bool> {
        let (width, height) = self.backend.size().map(|(w, h)| (usize::from(w), usize::from(h)))?;
        self.apply_size(width, height)
    }

    fn apply_size(&mut self, width: usize, height: usize) -> io::Result<bool> {
        if width == self.width && height == self.height {
            return Ok(false);
        }
        if width != self.width {
            self.cache.clear();
        }
        self.width = width;
        self.height = height;
        self.full_redraw()?;
        Ok(true)
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn rendered(&self) -> Option<&InteractiveLayout> {
        self.rendered.as_ref()
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.advance_spinner();
        self.check_system_message_expiration();
        self.redraw()
    }

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
        self.backend.hide_cursor()?;
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

    pub fn set_system_message(&mut self, message: impl Into<String>) {
        self.state.set_system_message(Some(message.into()));
        self.system_message_expires_at = Some(Instant::now() + SYSTEM_MESSAGE_DURATION);
    }

    pub fn clear_system_message(&mut self) {
        self.state.set_system_message(None);
        self.system_message_expires_at = None;
    }

    pub fn check_system_message_expiration(&mut self) -> bool {
        if let Some(expires_at) = self.system_message_expires_at
            && Instant::now() >= expires_at
        {
            self.clear_system_message();
            return true;
        }
        false
    }

    fn active_widget_lines(&self) -> Vec<String> {
        self.state.active_tool().map_or_else(Vec::new, |tool| {
            super::layout::render_running_tool_widget(super::layout::RunningToolWidgetInput {
                tool,
                theme: &self.theme,
                width: self.width,
                tools_expanded: self.state.tools_expanded(),
            })
        })
    }

    pub(super) fn current_layout(&self) -> InteractiveLayout {
        let queue_slice: Vec<super::QueuedMessage> = self.state.queue().iter().cloned().collect();
        let widget_lines = self.active_widget_lines();
        let editor = self
            .state
            .active_modal_saved_editor()
            .unwrap_or_else(|| self.state.editor());

        layout(LayoutInput {
            editor,
            modal: self.state.active_modal(),
            autocomplete: Some(&self.state.autocomplete),
            footer: self.state.footer(),
            system_message: self.state.system_message(),
            queued_messages: &queue_slice,
            widget_lines: &widget_lines,
            terminal_width: self.width,
            terminal_height: self.height,
            spinner_frame: self.spinner_frame,
            theme: Some(&self.theme),
            focused: self.focused,
        })
    }
}

impl<B: TerminalBackend> Drop for TerminalController<B> {
    fn drop(&mut self) {
        self.restore();
    }
}
