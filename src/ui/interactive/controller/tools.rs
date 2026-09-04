use std::io;

use super::TerminalController;
use super::backend::TerminalBackend;
use crate::ui::interactive::{Activity, RunningTool, ToolStartRequest};

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
}
