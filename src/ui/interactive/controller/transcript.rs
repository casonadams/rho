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

    pub fn set_theme(&mut self, theme: crate::ui::theme::Theme) -> io::Result<()> {
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

    pub fn block_style(&self) -> crate::ui::theme::BlockStyle {
        self.theme.block_style
    }

    pub fn set_block_style(&mut self, style: crate::ui::theme::BlockStyle) -> io::Result<crate::ui::theme::BlockStyle> {
        if self.theme.block_style == style {
            return Ok(style);
        }
        self.theme.block_style = style;
        self.cache.clear();
        self.redraw_transcript_or_live()?;
        Ok(style)
    }

    pub fn toggle_block_style(&mut self) -> io::Result<crate::ui::theme::BlockStyle> {
        let next = match self.theme.block_style {
            crate::ui::theme::BlockStyle::Solid => crate::ui::theme::BlockStyle::Border,
            crate::ui::theme::BlockStyle::Border => crate::ui::theme::BlockStyle::Solid,
        };
        self.set_block_style(next)
    }

    pub fn block_agent_output(&self) -> bool {
        self.theme.block_agent_output
    }

    pub fn set_block_agent_output(&mut self, enabled: bool) -> io::Result<bool> {
        if self.theme.block_agent_output == enabled {
            return Ok(enabled);
        }
        self.theme.block_agent_output = enabled;
        self.cache.clear();
        self.redraw_transcript_or_live()?;
        Ok(enabled)
    }

    pub fn toggle_block_agent_output(&mut self) -> io::Result<bool> {
        let next = !self.theme.block_agent_output;
        self.set_block_agent_output(next)
    }

    pub(super) fn redraw_transcript_or_live(&mut self) -> io::Result<()> {
        if self.transcript.is_empty() {
            self.redraw()
        } else {
            self.full_redraw()
        }
    }

    pub fn push_transcript_item(&mut self, item: TranscriptItem) -> io::Result<bool> {
        if matches!(item, TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_)) {
            self.commit_streamed_output();
        }
        if let TranscriptItem::Tool(ref tool) = item
            && self.state.active_tool().is_some()
        {
            self.commit_active_tool(tool.clone())?;
            return Ok(true);
        }
        if matches!(item, TranscriptItem::Tool(_)) {
            self.clear_active_tool();
        }
        let rendered = self.render_transcript_item(&item);
        self.cache.push(
            target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
            &rendered,
        );
        self.transcript.push(item);
        let is_streamed_content = matches!(
            self.transcript.last(),
            Some(TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_))
        );
        if !rendered.is_empty() && !is_streamed_content {
            self.write_output(&rendered)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn render_transcript_item(&self, item: &TranscriptItem) -> String {
        render_transcript_item(TranscriptRenderInput {
            item,
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
            hide_thinking: self.state.hide_thinking(),
        })
    }

    fn render_cached(&mut self, idx: usize, item: &TranscriptItem) -> String {
        self.cache
            .get_or_render(
                idx,
                TranscriptRenderInput {
                    item,
                    theme: &self.theme,
                    width: self.width,
                    tools_expanded: self.state.tools_expanded(),
                    hide_thinking: self.state.hide_thinking(),
                },
            )
            .to_string()
    }

    fn repaint_item(&mut self, idx: usize, item: &TranscriptItem, redraw_buffer: &mut String) -> io::Result<()> {
        let rendered = self.render_cached(idx, item);
        if rendered.is_empty() {
            return Ok(());
        }
        let formatted = terminal_newlines(&rendered);
        self.output.update(&formatted);
        redraw_buffer.push_str(&formatted);
        if self.output.is_open() {
            redraw_buffer.push_str("\r\n");
            self.output.update("\n");
        }
        Ok(())
    }

    fn repaint_history(&mut self, redraw_buffer: &mut String) -> io::Result<()> {
        let items = std::mem::take(&mut self.transcript);
        for (idx, item) in items.iter().enumerate() {
            self.repaint_item(idx, item, redraw_buffer)?;
        }
        self.transcript = items;
        Ok(())
    }

    fn repaint_streamed_output(&mut self, redraw_buffer: &mut String) {
        if self.streamed_output.is_empty() {
            return;
        }
        self.output.update(&self.streamed_output);
        redraw_buffer.push_str(&self.streamed_output);
        if self.output.is_open() {
            redraw_buffer.push_str("\r\n");
        }
    }

    pub fn full_redraw(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.rendered = None;
        self.backend.write_text(CSI_BEGIN_SYNC_UPDATE)?;

        let redraw_result = self.run_full_redraw();

        let _ = self.backend.write_text(CSI_END_SYNC_UPDATE);
        let flush_result = self.backend.flush();
        redraw_result.and(flush_result)
    }

    fn run_full_redraw(&mut self) -> io::Result<()> {
        self.backend.write_text("\x1b[2J\x1b[H\x1b[0m")?;
        self.output.clear();
        let mut redraw_buffer = String::new();
        self.repaint_history(&mut redraw_buffer)?;
        self.repaint_streamed_output(&mut redraw_buffer);
        if !redraw_buffer.is_empty() {
            self.backend.write_text(&redraw_buffer)?;
        }

        let rendered = self.current_layout();
        paint::write_live_region(&mut self.backend, &rendered)?;
        self.backend.hide_cursor()?;
        self.rendered = Some(rendered);
        Ok(())
    }
}
