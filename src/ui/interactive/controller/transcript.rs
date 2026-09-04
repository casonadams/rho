use std::io;

use super::TerminalController;
use super::ansi::terminal_newlines;
use super::backend::TerminalBackend;
use super::paint;
use crate::ui::interactive::{TranscriptItem, TranscriptRenderInput, render_transcript_item};

impl<B: TerminalBackend> TerminalController<B> {
    pub fn transcript(&self) -> &[TranscriptItem] {
        &self.transcript
    }

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
    }

    pub fn set_transcript(&mut self, items: Vec<TranscriptItem>) -> io::Result<()> {
        self.transcript = items;
        self.full_redraw()
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

    pub fn push_transcript_item(&mut self, item: TranscriptItem) -> io::Result<bool> {
        if matches!(item, TranscriptItem::Tool(_)) {
            self.clear_active_tool();
        }
        let is_streamed_assistant = matches!(item, TranscriptItem::AssistantText(_));
        let rendered = render_transcript_item(TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
            hide_thinking: self.state.hide_thinking(),
        });
        self.transcript.push(item);
        if !rendered.is_empty() && !is_streamed_assistant {
            self.write_output(&rendered)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn full_redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        paint::erase_live_region(&mut self.backend, self.rendered.as_ref())?;
        self.rendered = None;
        self.backend.write_text("\x1b[2J\x1b[H\x1b[3J")?;
        self.output.clear();

        let tools_expanded = self.state.tools_expanded();
        let hide_thinking = self.state.hide_thinking();
        let rendered_items: Vec<String> = self
            .transcript
            .iter()
            .map(|item| {
                render_transcript_item(TranscriptRenderInput {
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
            self.output.update(rendered);
            if self.output.is_open() {
                self.backend.write_text("\r\n")?;
                self.output.update("\n");
            }
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
}
