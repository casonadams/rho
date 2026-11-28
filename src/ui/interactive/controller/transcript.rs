use super::TerminalController;
use super::ansi::{
    CLEAR_SCREEN, CLEAR_SCREEN_AND_SCROLLBACK, CSI_BEGIN_SYNC_UPDATE, CSI_END_SYNC_UPDATE, terminal_newlines,
};
use super::backend::TerminalBackend;
use super::cache::{TranscriptRenderCache, target_slot};
use crate::ui::interactive::{TranscriptItem, TranscriptRenderInput, render_transcript_item};
use crate::ui::theme::{BlockStyle, CursorMode, Theme};
use std::io;

#[derive(Clone, Copy)]
enum RedrawScope {
    VisibleTail,
    Everything,
}

impl<B: TerminalBackend> TerminalController<B> {
    pub fn transcript(&self) -> &[TranscriptItem] {
        &self.transcript
    }

    pub fn cache(&self) -> &TranscriptRenderCache {
        &self.cache
    }

    pub fn set_transcript(&mut self, items: Vec<TranscriptItem>) -> io::Result<()> {
        self.transcript = items;
        self.cache.clear();
        self.full_redraw()
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: Theme) -> io::Result<()> {
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

    fn set_theme_field<T: Copy + PartialEq>(&mut self, field: fn(&mut Theme) -> &mut T, value: T) -> io::Result<T> {
        let slot = field(&mut self.theme);
        if *slot == value {
            return Ok(value);
        }
        *slot = value;
        self.cache.clear();
        self.redraw_transcript_or_live()?;
        Ok(value)
    }

    pub fn block_style(&self) -> BlockStyle {
        self.theme.block_style
    }

    pub fn set_block_style(&mut self, style: BlockStyle) -> io::Result<BlockStyle> {
        self.set_theme_field(|theme| &mut theme.block_style, style)
    }

    pub fn toggle_block_style(&mut self) -> io::Result<BlockStyle> {
        let next = match self.theme.block_style {
            BlockStyle::Border => BlockStyle::Solid,
            BlockStyle::Solid => BlockStyle::Divider,
            BlockStyle::Divider => BlockStyle::Border,
        };
        self.set_block_style(next)
    }

    pub fn cursor_mode(&self) -> CursorMode {
        self.theme.cursor_mode
    }

    pub fn set_cursor_mode(&mut self, mode: CursorMode) -> io::Result<CursorMode> {
        self.set_theme_field(|theme| &mut theme.cursor_mode, mode)
    }

    pub fn toggle_cursor_mode(&mut self) -> io::Result<CursorMode> {
        let next = match self.theme.cursor_mode {
            CursorMode::Software => CursorMode::Hardware,
            CursorMode::Hardware => CursorMode::Software,
        };
        self.set_cursor_mode(next)
    }

    pub fn block_agent_output(&self) -> bool {
        self.theme.block_agent_output
    }

    pub fn set_block_agent_output(&mut self, enabled: bool) -> io::Result<bool> {
        self.set_theme_field(|theme| &mut theme.block_agent_output, enabled)
    }

    pub fn toggle_block_agent_output(&mut self) -> io::Result<bool> {
        self.set_block_agent_output(!self.theme.block_agent_output)
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
        let needs_prefix_newline =
            self.output.is_open() && !matches!(item, TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_));
        if let TranscriptItem::Tool(ref tool) = item
            && self.state.active_tool().is_some()
        {
            return self.commit_active_tool((**tool).clone(), needs_prefix_newline);
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
            self.write_transcript_card(&rendered, needs_prefix_newline)?;
            Ok(true)
        } else {
            if needs_prefix_newline {
                self.write_output("\n")?;
            }
            Ok(false)
        }
    }

    pub fn upsert_trailing_recap(&mut self, text: String) -> io::Result<bool> {
        let item = TranscriptItem::Recap(text);
        if matches!(self.transcript.last(), Some(TranscriptItem::Recap(_))) {
            let idx = self.transcript.len() - 1;
            let rendered = self.render_transcript_item(&item);
            let mut entry = super::cache::CachedItemRender::default();
            entry.set(
                target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
                rendered,
            );
            self.cache.set(idx, entry);
            self.transcript[idx] = item;
            self.full_redraw()?;
            Ok(true)
        } else {
            self.push_transcript_item(item)
        }
    }

    pub(crate) fn write_transcript_card(&mut self, rendered: &str, needs_prefix_newline: bool) -> io::Result<()> {
        if needs_prefix_newline {
            let mut combined = String::with_capacity(rendered.len() + 1);
            combined.push('\n');
            combined.push_str(rendered);
            self.write_output(&combined)
        } else {
            self.write_output(rendered)
        }
    }

    fn render_transcript_item(&self, item: &TranscriptItem) -> String {
        self.render_detached(item, self.width)
    }

    /// Renders `item` the way the transcript would at `width`, without adding it to the transcript.
    pub fn render_detached(&self, item: &TranscriptItem, width: usize) -> String {
        render_transcript_item(TranscriptRenderInput {
            item,
            theme: &self.theme,
            width,
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

    fn repaint_item(&mut self, idx: usize, item: &TranscriptItem, redraw_buffer: &mut String) {
        let rendered = self.render_cached(idx, item);
        if rendered.is_empty() {
            return;
        }
        let formatted = terminal_newlines(&rendered);
        self.output.update(&formatted);
        redraw_buffer.push_str(&formatted);
        if self.output.is_open() {
            redraw_buffer.push_str("\r\n");
            self.output.update("\n");
        }
    }

    fn visible_tail_start(&mut self, items: &[TranscriptItem], rows: usize) -> usize {
        if rows == 0 {
            return items.len();
        }
        let mut used = 0;
        for idx in (0..items.len()).rev() {
            let rendered = self.render_cached(idx, &items[idx]);
            if rendered.is_empty() {
                continue;
            }
            let lines = rendered.split('\n').count();
            if used + lines > rows {
                return if used == 0 || self.pin.is_some() { idx } else { idx + 1 };
            }
            used += lines;
        }
        0
    }

    fn repaint_history(&mut self, redraw_buffer: &mut String, scope: RedrawScope, rows: usize) {
        let items = std::mem::take(&mut self.transcript);
        let start = match scope {
            RedrawScope::VisibleTail => self.visible_tail_start(&items, rows),
            RedrawScope::Everything => 0,
        };
        for (idx, item) in items.iter().enumerate().skip(start) {
            self.repaint_item(idx, item, redraw_buffer);
        }
        self.transcript = items;
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
        self.synchronized_redraw(RedrawScope::VisibleTail)
    }

    pub fn replay_transcript(&mut self) -> io::Result<()> {
        self.synchronized_redraw(RedrawScope::Everything)
    }

    fn synchronized_redraw(&mut self, scope: RedrawScope) -> io::Result<()> {
        self.backend.hide_cursor()?;
        self.rendered = None;
        self.backend.write_text(CSI_BEGIN_SYNC_UPDATE)?;

        let redraw_result = self.run_full_redraw(scope);

        let _ = self.backend.write_text(CSI_END_SYNC_UPDATE);
        let flush_result = self.backend.flush();
        redraw_result.and(flush_result)
    }

    fn run_full_redraw(&mut self, scope: RedrawScope) -> io::Result<()> {
        let region = self.current_layout();
        let clear = match scope {
            RedrawScope::VisibleTail => CLEAR_SCREEN,
            RedrawScope::Everything => CLEAR_SCREEN_AND_SCROLLBACK,
        };
        self.backend.write_text(clear)?;
        self.output.clear();
        let mut redraw_buffer = String::new();
        let rows = self.height.saturating_sub(region.height());
        self.repaint_history(&mut redraw_buffer, scope, rows);
        self.repaint_streamed_output(&mut redraw_buffer);
        if !redraw_buffer.is_empty() {
            self.backend.write_text(&redraw_buffer)?;
        }

        let pad = self.settle_pin(&redraw_buffer, region.height());
        self.draw_region(region, pad)
    }

    /// Records what a redraw printed on the cleared screen and returns the pad for the region.
    fn settle_pin(&mut self, printed: &str, region_height: usize) -> usize {
        let open = self.output.is_open();
        let (width, height) = (self.width, self.height);
        let Some(pin) = self.pin.as_mut() else {
            return 0;
        };
        pin.clear(width);
        pin.print(if open {
            printed.strip_suffix("\r\n").unwrap_or(printed)
        } else {
            printed
        });
        pin.settle(region_height, height)
    }
}
