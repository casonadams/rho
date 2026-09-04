use super::TerminalController;
use super::ansi::{CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE, terminal_newlines};
use super::backend::TerminalBackend;
use super::cache::{TranscriptRenderCache, target_slot};
use super::paint;
use crate::ui::interactive::{TranscriptItem, TranscriptRenderInput, render_transcript_item};
use std::io;

impl<B: TerminalBackend> TerminalController<B> {
    pub fn transcript(&self) -> &[TranscriptItem] {
        &self.transcript
    }

    pub fn cache(&self) -> &TranscriptRenderCache {
        &self.cache
    }

    pub fn clear_transcript(&mut self) {
        self.transcript.clear();
        self.cache.clear();
    }

    pub fn set_transcript(&mut self, items: Vec<TranscriptItem>) -> io::Result<()> {
        self.transcript = items;
        self.cache.clear();
        self.full_redraw()
    }

    pub fn theme(&self) -> &crate::ui::theme::Theme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: crate::ui::theme::Theme) -> io::Result<()> {
        if theme.is_ansi() {
            let _ = self.backend.write_text("\x1b]111\x1b\\\x1b]110\x1b\\");
        } else {
            if let Some(bg) = &theme.terminal_bg {
                let _ = self.backend.write_text(&format!("\x1b]11;{bg}\x1b\\"));
            }
            if let Some(fg) = &theme.terminal_fg {
                let _ = self.backend.write_text(&format!("\x1b]10;{fg}\x1b\\"));
            }
        }
        self.theme = theme;
        self.cache.clear();
        self.full_redraw()
    }

    pub fn tools_expanded(&self) -> bool {
        self.state.tools_expanded()
    }

    pub fn set_tools_expanded(&mut self, expanded: bool) -> io::Result<bool> {
        if self.state.tools_expanded() == expanded {
            return Ok(expanded);
        }
        self.state_mut().set_tools_expanded(expanded);
        self.redraw_transcript_or_live()?;
        Ok(expanded)
    }

    pub fn toggle_tools_expanded(&mut self) -> io::Result<bool> {
        self.set_tools_expanded(!self.state.tools_expanded())
    }

    pub fn hide_thinking(&self) -> bool {
        self.state.hide_thinking()
    }

    pub fn set_hide_thinking(&mut self, hide: bool) -> io::Result<bool> {
        if self.state.hide_thinking() == hide {
            return Ok(hide);
        }
        self.state_mut().set_hide_thinking(hide);
        self.redraw_transcript_or_live()?;
        Ok(hide)
    }

    pub fn toggle_thinking(&mut self) -> io::Result<bool> {
        self.set_hide_thinking(!self.state.hide_thinking())
    }

    pub(super) fn redraw_transcript_or_live(&mut self) -> io::Result<()> {
        if self.transcript.is_empty() {
            self.redraw()
        } else {
            self.full_redraw()
        }
    }

    pub fn push_transcript_item(&mut self, item: TranscriptItem) -> io::Result<bool> {
        if matches!(item, TranscriptItem::Tool(_)) {
            self.clear_active_tool();
        }
        let is_streamed_assistant = matches!(item, TranscriptItem::AssistantText(_));
        let tools_expanded = self.state.tools_expanded();
        let hide_thinking = self.state.hide_thinking();
        let rendered = render_transcript_item(TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded,
            hide_thinking,
        });
        self.cache
            .push(target_slot(&item, tools_expanded, hide_thinking), &rendered);
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
        self.backend.write_text(CSI_BEGIN_SYNC_UPDATE)?;

        let redraw_result = (|| -> io::Result<()> {
            self.backend.write_text("\x1b[2J\x1b[H\x1b[3J")?;
            self.output.clear();

            let (tools_expanded, hide_thinking) = (self.state.tools_expanded(), self.state.hide_thinking());
            let mut redraw_buffer = String::new();

            for (idx, item) in self.transcript.iter().enumerate() {
                let rendered = self.cache.get_or_render(
                    idx,
                    TranscriptRenderInput {
                        item,
                        theme: &self.theme,
                        width: self.width,
                        tools_expanded,
                        hide_thinking,
                    },
                );
                if !rendered.is_empty() {
                    let formatted = terminal_newlines(rendered);
                    self.output.update(&formatted);
                    redraw_buffer.push_str(&formatted);
                    if self.output.is_open() {
                        redraw_buffer.push_str("\r\n");
                        self.output.update("\n");
                    }
                }
            }

            if !redraw_buffer.is_empty() {
                self.backend.write_text(&redraw_buffer)?;
            }

            let rendered = self.current_layout();
            paint::write_live_region(&mut self.backend, &rendered)?;
            if rendered.cursor_visible {
                self.backend.show_cursor()?;
            } else {
                self.backend.hide_cursor()?;
            }
            self.rendered = Some(rendered);
            Ok(())
        })();

        let _ = self.backend.write_text(CSI_END_SYNC_UPDATE);
        redraw_result?;
        self.backend.flush()
    }
}
