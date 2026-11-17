use std::io;

use super::TerminalController;
use super::backend::TerminalBackend;
use crate::ui::interactive::{Activity, RunningTool, ToolItem, ToolStartRequest, TranscriptItem};
use crate::ui::render::card::render_tool_block;

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

    pub fn commit_active_tool(&mut self, tool: ToolItem, needs_prefix_newline: bool) -> io::Result<bool> {
        let expanded = self.state.tools_expanded();
        let block = render_tool_block(&tool, self.width, expanded, expanded, &self.theme);
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
        self.write_transcript_card(&rendered, needs_prefix_newline)?;
        Ok(true)
    }
}
