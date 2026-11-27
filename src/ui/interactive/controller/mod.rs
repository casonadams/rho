pub mod ansi;
pub mod backend;
pub mod cache;
pub mod paint;
mod pin;
mod sgr;
#[cfg(test)]
pub(crate) mod tests;
pub mod title;
pub mod tools;
pub mod transcript;
pub mod view;

use crate::ui::theme::SGR_RESET;
pub use ansi::OutputTracker;
use ansi::terminal_newlines;
#[cfg(test)]
pub(crate) use backend::NullBackend;
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
    /// Another agent shown in place of the main agent.
    pub(super) view: Option<view::AgentView>,
    pub(super) approval_focus: rho_harness_core::config::ApprovalFocus,
    /// Present when the live region is kept on the last rows of the screen.
    pin: Option<pin::BottomPin>,
    title: title::TerminalTitleController,
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

    /// Like `stdout`, with the live region kept on the last rows of the screen.
    pub fn stdout_pinned(state: crate::ui::interactive::InteractiveState) -> io::Result<Self> {
        Self::pinned_to_bottom(CrosstermBackend::stdout(), state)
    }
}

fn fit_pin(pin: &mut Option<pin::BottomPin>, region: &InteractiveLayout, screen: usize) -> pin::Fit {
    pin.as_mut()
        .map_or(pin::Fit::Pad(0), |pin| pin.fit(region.height(), screen))
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(backend: B, state: InteractiveState) -> io::Result<Self> {
        Self::build(backend, state, false)
    }

    /// A controller whose live region stays on the last rows of the screen: the visible screen is
    /// first scrolled into the scrollback, so it starts blank with the region at the bottom.
    pub fn pinned_to_bottom(backend: B, state: InteractiveState) -> io::Result<Self> {
        Self::build(backend, state, true)
    }

    fn build(mut backend: B, state: InteractiveState, pinned: bool) -> io::Result<Self> {
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
            view: None,
            approval_focus: rho_harness_core::config::ApprovalFocus::default(),
            pin: pinned.then(|| pin::BottomPin::new(width)),
            title: title::TerminalTitleController::default(),
        };
        if let Err(error) = controller.start() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    fn start(&mut self) -> io::Result<()> {
        if self.pin.is_some() {
            self.backend.move_down(self.height)?;
            self.backend.write_text(&"\r\n".repeat(self.height))?;
            self.backend.write_text(ansi::CURSOR_HOME)?;
        }
        self.redraw()
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// What happens when an agent other than the one shown asks for a permission answer.
    pub fn approval_focus(&self) -> rho_harness_core::config::ApprovalFocus {
        self.approval_focus
    }

    pub fn set_approval_focus(&mut self, focus: rho_harness_core::config::ApprovalFocus) {
        self.approval_focus = focus;
    }

    pub fn configure_terminal_title(&mut self, enabled: bool, show_state: bool) -> io::Result<()> {
        self.title
            .configure(&mut self.backend, enabled, show_state, &self.state)
    }

    pub fn set_attention(&mut self, attention: bool) {
        self.title.set_attention(attention);
    }

    fn pad(&self) -> usize {
        self.pin.as_ref().map_or(0, pin::BottomPin::pad)
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        let prev_pad = self.pad();
        let region = self.current_layout();
        match fit_pin(&mut self.pin, &region, self.height) {
            pin::Fit::Pad(pad) => {
                {
                    let prev = self.rendered.as_ref().map(|prev| prev.with_pad(prev_pad));
                    paint::render_live_diff(&mut self.backend, prev.as_deref(), &region.with_pad(pad))?;
                }
                self.rendered = Some(region);
            }
            pin::Fit::Repaint => {
                self.backend.write_text(ansi::CSI_BEGIN_SYNC_UPDATE)?;
                self.backend.hide_cursor()?;
                self.repaint_from_rows(region)?;
                self.backend.write_text(ansi::CSI_END_SYNC_UPDATE)?;
                self.backend.flush()?;
            }
        }
        self.title.sync(&mut self.backend, &self.state)?;
        Ok(())
    }

    fn erase_region(&mut self) -> io::Result<()> {
        let pad = self.pad();
        let painted = self.rendered.as_ref().map(|region| region.with_pad(pad));
        paint::erase_live_region(&mut self.backend, painted.as_deref())
    }

    /// Draws the region under `pad` blank rows at the cursor.
    pub(super) fn draw_region(&mut self, region: InteractiveLayout, pad: usize) -> io::Result<()> {
        {
            let painted = region.with_pad(pad);
            paint::write_live_region(&mut self.backend, &painted)?;
            paint::apply_cursor_visibility(&mut self.backend, &painted)?;
        }
        self.rendered = Some(region);
        Ok(())
    }

    /// Clears the screen, prints the newest printed rows that fit above the region, and draws the
    /// region under whatever blank rows remain.
    fn repaint_from_rows(&mut self, region: InteractiveLayout) -> io::Result<()> {
        let Some(pin) = self.pin.as_mut() else {
            return Ok(());
        };
        let rows = pin.repaint_rows(region.height(), self.height);
        let pad = pin.pad();
        self.backend.write_text(ansi::CLEAR_SCREEN)?;
        for row in &rows {
            self.backend.write_text(row)?;
            self.backend.write_text(SGR_RESET)?;
            self.backend.write_text("\r\n")?;
        }
        self.draw_region(region, pad)
    }

    fn prepare_output_write(&mut self) -> io::Result<()> {
        self.backend.write_text(ansi::CSI_BEGIN_SYNC_UPDATE)?;
        self.backend.hide_cursor()?;
        self.erase_region()?;
        self.rendered = None;
        self.output.restore_cursor(&mut self.backend, self.width)
    }

    fn finish_output_write(&mut self) -> io::Result<()> {
        let region = self.current_layout();
        match fit_pin(&mut self.pin, &region, self.height) {
            pin::Fit::Pad(pad) => self.draw_region(region, pad)?,
            pin::Fit::Repaint => self.repaint_from_rows(region)?,
        }
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
        let output = if prefix_newline {
            std::borrow::Cow::Owned(format!("\r\n{output}"))
        } else {
            std::borrow::Cow::Borrowed(output)
        };
        let visible = if self.output.ends_at_wrap_boundary(self.width) {
            output.strip_prefix("\r\n").unwrap_or(&output)
        } else {
            &output
        };
        self.prepare_output_write()?;
        if !self.output.style().is_empty() {
            self.backend.write_text(self.output.style())?;
        }
        self.backend.write_text(visible)?;
        self.output.update(&output);
        if let Some(pin) = self.pin.as_mut() {
            pin.print(&output);
        }
        if !self.output.style().is_empty() {
            self.backend.write_text(SGR_RESET)?;
        }
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
        self.replay_transcript()?;
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
        self.erase_region()?;
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
        if self.pin.is_some() {
            self.full_redraw()
        } else {
            self.redraw()
        }
    }

    pub(super) fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.erase_region();
        self.rendered = None;
        self.title.dispose(&mut self.backend);
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
        let mut lines = self.state.active_tool().map_or_else(Vec::new, |tool| {
            super::layout::render_running_tool_widget(super::layout::RunningToolWidgetInput {
                tool,
                theme: &self.theme,
                width: self.width,
                tools_expanded: self.state.tools_expanded(),
            })
        });
        if self.state.a2ui_widgets() && self.state.active_modal().is_none() {
            let surfaces = self
                .state
                .a2ui_surfaces_for(rho_harness_core::a2ui::SurfacePlacement::AboveEditor);
            lines.extend(super::a2ui::render_surfaces(
                &surfaces,
                self.width,
                self.state.a2ui_config().max_above_lines,
                &self.theme,
            ));
        }
        lines
    }

    fn below_widget_lines(&self) -> Vec<String> {
        if !self.state.a2ui_widgets() || self.state.active_modal().is_some() {
            return Vec::new();
        }
        let surfaces = self
            .state
            .a2ui_surfaces_for(rho_harness_core::a2ui::SurfacePlacement::BelowEditor);
        super::a2ui::render_surfaces(
            &surfaces,
            self.width,
            self.state.a2ui_config().max_below_lines,
            &self.theme,
        )
    }

    pub(super) fn current_layout(&self) -> InteractiveLayout {
        let queue_slice: Vec<super::QueuedMessage> = self.state.queue().iter().cloned().collect();
        let widget_lines = self.active_widget_lines();
        let below_widget_lines = self.below_widget_lines();
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
            below_widget_lines: &below_widget_lines,
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
