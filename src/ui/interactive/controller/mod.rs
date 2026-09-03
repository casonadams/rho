pub mod ansi;
pub mod backend;
#[cfg(test)]
mod tests;

pub use ansi::{output_cursor, terminal_newlines};
pub use backend::{CrosstermBackend, TerminalBackend};

use super::{InteractiveLayout, InteractiveState, LayoutInput, layout};
use std::io;

pub struct TerminalController<B: TerminalBackend> {
    backend: B,
    state: InteractiveState,
    width: usize,
    rendered: Option<InteractiveLayout>,
    previous_lines: Vec<String>,
    current_cursor_row: usize,
    output_line: String,
    output_line_open: bool,
    spinner_frame: usize,
    active: bool,
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
            previous_lines: Vec::new(),
            current_cursor_row: 0,
            output_line: String::new(),
            output_line_open: false,
            spinner_frame: 0,
            active: true,
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
        self.state.footer_mut().running_tool = Some(request.name.clone());
        self.state.footer_mut().activity = crate::ui::interactive::Activity::Working;
        self.state.set_active_tool(Some(super::RunningTool::new(
            request.name,
            request.args_summary,
            request.preview,
        )));
        self.redraw()
    }

    pub fn append_tool_chunk(&mut self, chunk: &str) -> io::Result<()> {
        if let Some(tool) = self.state.active_tool_mut() {
            tool.append_chunk(chunk);
            self.redraw()?;
        }
        Ok(())
    }

    pub fn append_tool_chunks<'chunk, I: IntoIterator<Item = &'chunk str>>(&mut self, chunks: I) -> io::Result<()> {
        if let Some(tool) = self.state.active_tool_mut() {
            let mut any = false;
            for chunk in chunks {
                tool.append_chunk(chunk);
                any = true;
            }
            if any {
                self.redraw()?;
            }
        }
        Ok(())
    }

    pub fn clear_active_tool(&mut self) {
        self.state.footer_mut().running_tool = None;
        self.state.set_active_tool(None);
    }

    pub fn end_tool(&mut self) -> io::Result<()> {
        let had_active = self.state.active_tool().is_some() || self.state.footer().running_tool.is_some();
        self.state.footer_mut().running_tool = None;
        self.state.set_active_tool(None);
        if had_active {
            self.redraw()?;
        }
        Ok(())
    }

    pub fn push_transcript_item(&mut self, item: super::TranscriptItem) -> io::Result<()> {
        if matches!(item, super::TranscriptItem::Tool(_)) {
            self.clear_active_tool();
        }
        let is_streamed_assistant = matches!(item, super::TranscriptItem::AssistantText(_));
        let rendered = super::render_transcript_item(super::TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
            hide_thinking: self.state.hide_thinking(),
        });
        self.transcript.push(item);
        if !rendered.is_empty() && !is_streamed_assistant {
            self.write_output(&rendered)?;
        }
        Ok(())
    }

    pub fn full_redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.backend.write_text("\x1b[2J\x1b[H\x1b[3J")?;
        self.output_line.clear();
        self.output_line_open = false;

        let tools_expanded = self.state.tools_expanded();
        let hide_thinking = self.state.hide_thinking();
        let rendered_items: Vec<String> = self
            .transcript
            .iter()
            .map(|item| {
                super::render_transcript_item(super::TranscriptRenderInput {
                    item,
                    theme: &self.theme,
                    width: self.width,
                    tools_expanded,
                    hide_thinking,
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

        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        let cursor_visible = rendered.cursor_visible;
        self.rendered = Some(rendered);
        if cursor_visible {
            self.backend.show_cursor()?;
        } else {
            self.backend.hide_cursor()?;
        }
        self.backend.flush()
    }

    pub fn state(&self) -> &InteractiveState {
        &self.state
    }

    pub fn transcript(&self) -> &[super::TranscriptItem] {
        &self.transcript
    }

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
    }

    pub fn set_transcript(&mut self, items: Vec<super::TranscriptItem>) -> io::Result<()> {
        self.transcript = items;
        self.full_redraw()
    }

    pub fn state_mut(&mut self) -> &mut InteractiveState {
        &mut self.state
    }

    pub fn terminal_width(&self) -> usize {
        self.width.max(1)
    }

    pub fn redraw(&mut self) -> io::Result<()> {
        let rendered = self.current_layout();
        let next_lines = &rendered.lines;
        let new_height = next_lines.len();
        let target_cursor_row = rendered.cursor_row();
        let target_cursor_col = rendered.cursor.column;

        self.backend.hide_cursor()?;

        if let Some(prev) = &self.rendered {
            let prev_height = prev.height();
            let prev_cursor_row = prev.cursor_row();

            if prev_cursor_row > 0 {
                self.backend.move_up(prev_cursor_row)?;
            }
            self.backend.move_to_column(0)?;

            for (i, line) in next_lines.iter().enumerate() {
                if i > 0 {
                    if i < prev_height {
                        self.backend.move_to_column(0)?;
                        self.backend.move_down(1)?;
                    } else {
                        self.backend.write_text("\r\n")?;
                    }
                }
                self.backend.clear_line()?;
                self.backend.write_text(line)?;
            }

            if prev_height > new_height {
                for _ in new_height..prev_height {
                    self.backend.move_to_column(0)?;
                    self.backend.move_down(1)?;
                    self.backend.clear_line()?;
                }
                let rows_up = (prev_height.saturating_sub(1)).saturating_sub(target_cursor_row);
                if rows_up > 0 {
                    self.backend.move_to_column(0)?;
                    self.backend.move_up(rows_up)?;
                }
            } else {
                let rows_up = (new_height.saturating_sub(1)).saturating_sub(target_cursor_row);
                if rows_up > 0 {
                    self.backend.move_to_column(0)?;
                    self.backend.move_up(rows_up)?;
                }
            }
            self.backend.move_to_column(target_cursor_col)?;
        } else {
            for (i, line) in next_lines.iter().enumerate() {
                if i > 0 {
                    self.backend.write_text("\r\n")?;
                }
                self.backend.write_text(line)?;
            }
            let rows_up = (new_height.saturating_sub(1)).saturating_sub(target_cursor_row);
            if rows_up > 0 {
                self.backend.move_to_column(0)?;
                self.backend.move_up(rows_up)?;
            }
            self.backend.move_to_column(target_cursor_col)?;
        }

        self.previous_lines = rendered.lines.clone();
        self.current_cursor_row = target_cursor_row;
        let cursor_visible = rendered.cursor_visible;
        self.rendered = Some(rendered);
        if cursor_visible {
            self.backend.show_cursor()?;
        } else {
            self.backend.hide_cursor()?;
        }
        self.backend.flush()
    }

    pub fn write_output(&mut self, output: &str) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.erase_live_region()?;
        self.restore_output_cursor()?;
        let output = terminal_newlines(output);
        self.backend.write_text(&output)?;
        self.update_output_line(&output);
        if self.output_line_open {
            self.backend.write_text("\r\n")?;
        }
        let rendered = self.current_layout();
        self.write_live_region(&rendered)?;
        self.previous_lines = rendered.lines.clone();
        self.current_cursor_row = rendered.cursor_row();
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

    pub fn toggle_thinking(&mut self) -> io::Result<bool> {
        let hide_thinking = self.state_mut().toggle_thinking();
        if self.transcript.is_empty() {
            self.redraw()?;
        } else {
            self.full_redraw()?;
        }
        Ok(hide_thinking)
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
            queued_messages: &queue_slice,
            widget_lines: &widget_lines,
            terminal_width: self.width,
            spinner_frame: self.spinner_frame,
        })
    }

    fn write_live_region(&mut self, rendered: &InteractiveLayout) -> io::Result<()> {
        let total = rendered.lines.len();
        for (i, line) in rendered.lines.iter().enumerate() {
            self.backend.write_text(line)?;
            if i + 1 < total {
                self.backend.write_text("\r\n")?;
            }
        }
        let rows_up = (total.saturating_sub(1)).saturating_sub(rendered.cursor_row());
        if rows_up > 0 {
            self.backend.move_to_column(0)?;
            self.backend.move_up(rows_up)?;
        }
        self.backend.move_to_column(rendered.cursor.column)
    }

    fn erase_live_region(&mut self) -> io::Result<()> {
        let Some(rendered) = self.rendered.as_ref() else {
            return Ok(());
        };
        let height = rendered.height();
        let cursor_row = rendered.cursor_row();
        if height > 0 {
            let rows_down = (height.saturating_sub(1)).saturating_sub(cursor_row);
            self.backend.move_to_column(0)?;
            if rows_down > 0 {
                self.backend.move_down(rows_down)?;
            }
            for row in (0..height).rev() {
                self.backend.clear_line()?;
                if row > 0 {
                    self.backend.move_up(1)?;
                }
            }
            self.backend.move_to_column(0)?;
        }
        self.rendered = None;
        Ok(())
    }

    fn restore(&mut self) {
        if !self.active {
            return;
        }
        let _ = self.erase_live_region();
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
