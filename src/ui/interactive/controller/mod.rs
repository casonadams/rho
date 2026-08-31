pub mod active_tool;
pub mod ansi;
pub mod backend;
#[cfg(test)]
mod tests;

pub use active_tool::{ActiveToolBlock, MAX_ACTIVE_TOOL_OUTPUT_BYTES};
pub use ansi::{output_cursor, terminal_newlines};
pub use backend::{CrosstermBackend, TerminalBackend};

use super::{InteractiveLayout, InteractiveState, LayoutInput, layout};
use anstyle::Style;
use std::io;

pub struct TerminalController<B: TerminalBackend> {
    backend: B,
    state: InteractiveState,
    width: usize,
    rendered: Option<InteractiveLayout>,
    output_line: String,
    output_line_open: bool,
    spinner_frame: usize,
    active: bool,
    footer_style: Style,
    active_tool: Option<ActiveToolBlock>,
    active_tool_height: usize,
    theme: crate::ui::theme::Theme,
    transcript: Vec<super::TranscriptItem>,
}

impl TerminalController<CrosstermBackend> {
    pub fn stdout(state: InteractiveState) -> io::Result<Self> {
        Self::new(CrosstermBackend::stdout(), state)
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn new(mut backend: B, state: InteractiveState) -> io::Result<Self> {
        backend.set_raw_mode(true)?;
        let width = match backend.size() {
            Ok((width, _)) => usize::from(width),
            Err(error) => {
                let _ = backend.set_raw_mode(false);
                return Err(error);
            }
        };
        let mut controller = Self {
            backend,
            state,
            width,
            rendered: None,
            output_line: String::new(),
            output_line_open: false,
            spinner_frame: 0,
            active: true,
            footer_style: crate::ui::theme::Theme::default().dimmed,
            active_tool: None,
            active_tool_height: 0,
            theme: crate::ui::theme::Theme::default(),
            transcript: Vec::new(),
        };
        if let Err(error) = controller.redraw() {
            controller.restore();
            return Err(error);
        }
        Ok(controller)
    }

    pub fn start_tool(&mut self, request: super::ToolStartRequest) -> io::Result<()> {
        self.active_tool = Some(ActiveToolBlock {
            name: request.name,
            args_summary: request.args_summary,
            preview: request.preview,
            output: String::new(),
            started: std::time::Instant::now(),
            truncated: false,
        });
        self.redraw()
    }

    pub fn append_tool_chunk(&mut self, chunk: &str) -> io::Result<()> {
        self.append_tool_chunks(std::iter::once(chunk))
    }

    /// Append several chunks and redraw once; a redraw per chunk would
    /// re-wrap the whole live block per chunk.
    pub fn append_tool_chunks<'chunk, I: IntoIterator<Item = &'chunk str>>(&mut self, chunks: I) -> io::Result<()> {
        let mut changed = false;
        if let Some(tool) = self.active_tool.as_mut() {
            for chunk in chunks {
                if tool.truncated {
                    break;
                }
                let room = MAX_ACTIVE_TOOL_OUTPUT_BYTES.saturating_sub(tool.output.len());
                let take = room.min(chunk.len());
                let take = if take == chunk.len() || chunk.is_char_boundary(take) {
                    take
                } else {
                    (0..take).rev().find(|&i| chunk.is_char_boundary(i)).unwrap_or(0)
                };
                if take > 0
                    && let Some(prefix) = chunk.get(..take)
                {
                    tool.output.push_str(prefix);
                    changed = true;
                }
                if tool.output.len() >= MAX_ACTIVE_TOOL_OUTPUT_BYTES {
                    tool.truncated = true;
                    tool.output
                        .push_str("\n[output truncated while running; full result follows]");
                }
            }
        }
        if changed { self.redraw() } else { Ok(()) }
    }

    pub fn clear_active_tool(&mut self) {
        self.active_tool = None;
    }

    pub fn end_tool(&mut self) -> io::Result<()> {
        self.active_tool = None;
        self.redraw()
    }

    pub fn push_transcript_item(&mut self, item: super::TranscriptItem) -> io::Result<()> {
        let rendered = super::render_transcript_item(super::TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
        });
        self.transcript.push(item);
        if !rendered.is_empty() {
            self.write_output(&rendered)?;
        }
        Ok(())
    }

    pub fn full_redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.backend.write_text("\x1b[2J\x1b[H\x1b[3J")?;
        self.output_line.clear();
        self.output_line_open = false;

        let tools_expanded = self.state.tools_expanded();
        let rendered_items: Vec<String> = self
            .transcript
            .iter()
            .map(|item| {
                super::render_transcript_item(super::TranscriptRenderInput {
                    item,
                    theme: &self.theme,
                    width: self.width,
                    tools_expanded,
                })
            })
            .filter(|rendered| !rendered.is_empty())
            .map(|rendered| terminal_newlines(&rendered))
            .collect();

        for rendered in &rendered_items {
            self.backend.write_text(rendered)?;
            self.update_output_line(rendered);
            if self.output_line_open {
                self.backend.write_text("\r\n")?;
            }
        }

        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn state(&self) -> &InteractiveState {
        &self.state
    }

    pub fn transcript(&self) -> &[super::TranscriptItem] {
        &self.transcript
    }

    pub fn state_mut(&mut self) -> &mut InteractiveState {
        &mut self.state
    }

    pub fn terminal_width(&self) -> usize {
        self.width.max(1)
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.restore_output_cursor()?;
        let output = terminal_newlines(output);
        self.backend.write_text(&output)?;
        self.update_output_line(&output);
        if self.output_line_open {
            self.backend.write_text("\r\n")?;
        }
        self.draw_active_tool()?;
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.rendered = Some(rendered);
        self.backend.show_cursor()?;
        self.backend.flush()
    }

    pub fn refresh_size(&mut self) -> io::Result<bool> {
        let (width, _) = self.backend.size()?;
        let width = usize::from(width);
        if width == self.width {
            return Ok(false);
        }
        self.width = width;
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(true)
    }

    pub fn toggle_tools_expanded(&mut self) -> io::Result<bool> {
        let tools_expanded = self.state_mut().toggle_tools_expanded();
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(tools_expanded)
    }

    pub fn advance_spinner(&mut self) {
        self.spinner_frame = self.spinner_frame.wrapping_add(1);
    }

    pub fn tick(&mut self) -> io::Result<()> {
        self.advance_spinner();
        self.redraw()
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.erase_live_region()?;
        self.erase_active_tool()?;
        self.output_line.clear();
        self.output_line_open = false;
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

    fn restore_output_cursor(&mut self) -> io::Result<()> {
        if !self.output_line_open {
            return Ok(());
        }
        let (column, at_wrap_boundary) = output_cursor(&self.output_line, self.width);
        if !at_wrap_boundary {
            self.backend.move_up(1)?;
        }
        self.backend.move_to_column(column)
    }

    fn update_output_line(&mut self, output: &str) {
        if output.is_empty() {
            return;
        }
        if let Some(newline) = output.rfind('\n') {
            self.output_line.clear();
            self.output_line.push_str(&output[newline + 1..]);
        } else {
            self.output_line.push_str(output);
        }
        self.output_line_open = !output.ends_with('\n');
    }

    fn current_layout(&self) -> InteractiveLayout {
        let queue_slice: Vec<super::QueuedMessage> = self.state.queue().iter().cloned().collect();
        layout(LayoutInput {
            editor: self.state.editor(),
            modal: self.state.active_modal(),
            footer: self.state.footer(),
            queued_messages: &queue_slice,
            terminal_width: self.width,
            spinner_frame: self.spinner_frame,
        })
    }

    fn write_live_region(&mut self, rendered: &InteractiveLayout) -> io::Result<()> {
        for line in &rendered.queued_lines {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
        }
        if !rendered.working_line.is_empty() {
            self.backend.write_text("\r\n")?;
            self.backend.write_text(&rendered.working_line)?;
            self.backend.write_text("\r\n")?;
        }
        if !rendered.top_divider.is_empty() {
            self.backend.write_text(&rendered.top_divider)?;
            self.backend.write_text("\r\n")?;
        }
        for line in &rendered.editor_lines {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
        }
        if !rendered.bottom_divider.is_empty() {
            self.backend.write_text(&rendered.bottom_divider)?;
            self.backend.write_text("\r\n")?;
        }
        let footer_style = self.footer_style;
        for (i, line) in rendered.footer_lines.iter().enumerate() {
            if i > 0 {
                self.backend.write_text("\r\n")?;
            }
            self.backend
                .write_text(&format!("{footer_style}{line}{footer_style:#}"))?;
        }

        self.backend.move_up(rendered.height() - 1 - rendered.cursor_row())?;
        self.backend.move_to_column(rendered.cursor.column)
    }

    fn erase_live_region(&mut self) -> io::Result<()> {
        let Some(rendered) = self.rendered.as_ref() else {
            return Ok(());
        };
        let height = rendered.height();
        let cursor_row = rendered.cursor_row();
        self.backend.move_down(height - 1 - cursor_row)?;
        self.backend.move_to_column(0)?;
        for row in (0..height).rev() {
            self.backend.clear_line()?;
            if row > 0 {
                self.backend.move_up(1)?;
            }
        }
        self.backend.move_to_column(0)?;
        self.rendered = None;
        Ok(())
    }

    fn erase_active_tool(&mut self) -> io::Result<()> {
        if self.active_tool_height == 0 {
            return Ok(());
        }
        self.backend.move_up(self.active_tool_height)?;
        self.backend.move_to_column(0)?;
        for row in 0..self.active_tool_height {
            self.backend.clear_line()?;
            if row + 1 < self.active_tool_height {
                self.backend.move_down(1)?;
            }
        }
        self.backend.move_up(self.active_tool_height.saturating_sub(1))?;
        self.backend.move_to_column(0)?;
        self.active_tool_height = 0;
        Ok(())
    }

    fn draw_active_tool(&mut self) -> io::Result<()> {
        let Some(tool) = &self.active_tool else {
            self.active_tool_height = 0;
            return Ok(());
        };
        let formatted = super::layout::format_active_tool_block(super::ActiveToolDisplayInput {
            tool_name: &tool.name,
            args_summary: &tool.args_summary,
            preview: tool.preview.as_deref(),
            output: &tool.output,
            started: tool.started,
            theme: &self.theme,
            width: self.width,
            expanded: self.state.tools_expanded(),
        });
        let formatted = terminal_newlines(&formatted);
        let mut count = 0;
        for line in formatted.lines() {
            self.backend.write_text(line)?;
            self.backend.write_text("\r\n")?;
            count += 1;
        }
        self.active_tool_height = count;
        Ok(())
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.erase_live_region();
        let _ = self.erase_active_tool();
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
