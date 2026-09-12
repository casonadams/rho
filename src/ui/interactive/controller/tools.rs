use std::io;

use super::TerminalController;
use super::backend::TerminalBackend;
use crate::ui::interactive::{
    Activity, InteractiveLayout, RunningTool, ToolItem, ToolStartRequest, TranscriptItem, TranscriptRenderInput,
    render_tool_block,
};

impl<B: TerminalBackend> TerminalController<B> {
    pub fn start_tool(&mut self, request: ToolStartRequest) -> io::Result<()> {
        self.state.footer_mut().running_tool = Some(request.name.clone());
        self.state.footer_mut().activity = Activity::Working;
        self.state.set_active_tool(Some(RunningTool::new(
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

    fn render_tool_lines(&self, tool: &ToolItem) -> (String, Vec<String>) {
        let input = TranscriptRenderInput {
            item: &TranscriptItem::Tool(tool.clone()),
            theme: &self.theme,
            width: self.width,
            tools_expanded: self.state.tools_expanded(),
            hide_thinking: self.state.hide_thinking(),
        };
        let block = render_tool_block(tool, &input);
        let mut card_lines = if self.theme.block_style == crate::ui::theme::BlockStyle::Border {
            Vec::new()
        } else {
            vec![String::new()]
        };
        card_lines.extend(block.lines().map(String::from));
        (block, card_lines)
    }

    fn tool_layout(&self, card_lines: &[String]) -> InteractiveLayout {
        let queue: Vec<crate::ui::interactive::QueuedMessage> = self.state.queue().iter().cloned().collect();
        crate::ui::interactive::layout(crate::ui::interactive::LayoutInput {
            editor: self.state.editor(),
            modal: self.state.active_modal(),
            autocomplete: Some(&self.state.autocomplete),
            footer: self.state.footer(),
            system_message: self.state.system_message(),
            queued_messages: &queue,
            widget_lines: card_lines,
            terminal_width: self.width,
            terminal_height: self.height,
            spinner_frame: self.spinner_frame,
            theme: Some(&self.theme),
            focused: self.focused,
        })
    }

    fn record_completed_tool(&mut self, tool: ToolItem, block: &str) {
        let item = TranscriptItem::Tool(tool);
        let rendered = if self.theme.block_style == crate::ui::theme::BlockStyle::Border {
            block.to_string()
        } else {
            format!("\n{block}")
        };
        self.cache.push(
            super::cache::target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
            &rendered,
        );
        self.transcript.push(item);

        let formatted = super::ansi::terminal_newlines(&rendered);
        self.output.update(&formatted);
        if self.output.is_open() {
            self.output.update("\n");
        }
    }

    pub fn commit_active_tool(&mut self, tool: ToolItem) -> io::Result<()> {
        let (block, card_lines) = self.render_tool_lines(&tool);
        let budget =
            ((self.height as f64) * crate::ui::interactive::layout::budget::MAX_WIDGET_HEIGHT_RATIO).round() as usize;
        if card_lines.len() > budget || self.state.tools_expanded() {
            let item = TranscriptItem::Tool(tool);
            let rendered = if self.theme.block_style == crate::ui::theme::BlockStyle::Border {
                block
            } else {
                format!("\n{block}")
            };
            self.clear_active_tool();
            self.cache.push(
                super::cache::target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
                &rendered,
            );
            self.transcript.push(item);
            self.write_output(&rendered)?;
            return Ok(());
        }
        let completed_layout = self.tool_layout(&card_lines);
        super::paint::render_live_diff(&mut self.backend, self.rendered.as_ref(), &completed_layout)?;
        self.record_completed_tool(tool, &block);
        self.clear_active_tool();
        self.rendered = Some(self.current_layout());
        self.backend.flush()
    }
}
