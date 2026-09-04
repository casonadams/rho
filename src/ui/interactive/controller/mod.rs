pub mod ansi;
pub mod backend;
pub mod cache;
pub mod lifecycle;
pub mod output;
pub mod paint;
pub mod system_message;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod transcript;

use ansi::terminal_newlines;
pub use backend::{CrosstermBackend, TerminalBackend};
pub use output::OutputTracker;

use std::io;

use super::{InteractiveLayout, InteractiveState, LayoutInput, layout};

pub struct TerminalController<B: TerminalBackend> {
    pub(super) backend: B,
    pub(super) state: InteractiveState,
    pub(super) width: usize,
    pub(super) height: usize,
    pub(super) rendered: Option<InteractiveLayout>,
    pub(super) output: OutputTracker,
    pub(super) spinner_frame: usize,
    pub(super) active: bool,
    pub(super) theme: crate::ui::theme::Theme,
    pub(super) transcript: Vec<super::TranscriptItem>,
    pub(super) system_message_expires_at: Option<std::time::Instant>,
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(mut backend: B, state: InteractiveState) -> io::Result<Self> {
        backend.set_raw_mode(true)?;
        let (width, height) = match backend.size() {
            Ok((width, height)) => (usize::from(width), usize::from(height)),
            Err(error) => {
                let _ = backend.set_raw_mode(false);
                return Err(error);
            }
        };
        let mut controller = Self {
            backend,
            state,
            width,
            height,
            rendered: None,
            output: OutputTracker::new(),
            spinner_frame: 0,
            active: true,
            theme: crate::ui::theme::Theme::default(),
            transcript: Vec::new(),
            system_message_expires_at: None,
        };
        if let Err(error) = controller.redraw() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        let rendered = self.current_layout();
        paint::render_live_diff(&mut self.backend, self.rendered.as_ref(), &rendered)?;
        self.rendered = Some(rendered);
        Ok(())
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        self.backend.hide_cursor()?;
        paint::erase_live_region(&mut self.backend, self.rendered.as_ref())?;
        self.rendered = None;
        self.output.restore_cursor(&mut self.backend, self.width)?;
        let output = terminal_newlines(output);
        self.backend.write_text(&output)?;
        self.output.update(&output);
        if self.output.is_open() {
            self.backend.write_text("\r\n")?;
        }
        let rendered = self.current_layout();
        paint::write_live_region(&mut self.backend, &rendered)?;
        let cursor_visible = rendered.cursor_visible;
        self.rendered = Some(rendered);
        if cursor_visible {
            self.backend.show_cursor()?;
        } else {
            self.backend.hide_cursor()?;
        }
        self.backend.flush()
    }

    pub fn refresh_size(&mut self) -> io::Result<bool> {
        let (width, height) = self.backend.size()?;
        let (width, height) = (usize::from(width), usize::from(height));
        if width == self.width && height == self.height {
            return Ok(false);
        }
        self.width = width;
        self.height = height;
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(true)
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.advance_spinner();
        self.check_system_message_expiration();
        self.redraw()
    }

    pub(super) fn current_layout(&self) -> InteractiveLayout {
        let queue_slice: Vec<super::QueuedMessage> = self.state.queue().iter().cloned().collect();
        let widget_lines = if let Some(tool) = self.state.active_tool() {
            super::layout::render_running_tool_widget(super::layout::RunningToolWidgetInput {
                tool,
                theme: &self.theme,
                width: self.width,
                tools_expanded: self.state.tools_expanded(),
            })
        } else {
            Vec::new()
        };

        layout(LayoutInput {
            editor: self.state.editor(),
            modal: self.state.active_modal(),
            autocomplete: Some(&self.state.autocomplete),
            footer: self.state.footer(),
            system_message: self.state.system_message(),
            queued_messages: &queue_slice,
            widget_lines: &widget_lines,
            terminal_width: self.width,
            terminal_height: self.height,
            spinner_frame: self.spinner_frame,
        })
    }
}
