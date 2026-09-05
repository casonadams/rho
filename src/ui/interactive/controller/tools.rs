use std::io;

use super::TerminalController;
use super::backend::TerminalBackend;
use crate::ui::interactive::{
    Activity, RunningTool, ToolItem, ToolStartRequest, TranscriptItem, TranscriptRenderInput, render_tool_block,
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

    pub fn commit_active_tool(&mut self, tool: ToolItem) -> io::Result<()> {
        let tools_expanded = self.state.tools_expanded();
        let hide_thinking = self.state.hide_thinking();
        let item = TranscriptItem::Tool(tool.clone());
        let input = TranscriptRenderInput {
            item: &item,
            theme: &self.theme,
            width: self.width,
            tools_expanded,
            hide_thinking,
        };
        let block = render_tool_block(&tool, &input);
        let mut card_lines = vec![String::new()];
        card_lines.extend(block.lines().map(String::from));

        let queue_slice: Vec<crate::ui::interactive::QueuedMessage> = self.state.queue().iter().cloned().collect();
        let completed_layout = crate::ui::interactive::layout(crate::ui::interactive::LayoutInput {
            editor: self.state.editor(),
            modal: self.state.active_modal(),
            autocomplete: Some(&self.state.autocomplete),
            footer: self.state.footer(),
            system_message: self.state.system_message(),
            queued_messages: &queue_slice,
            widget_lines: &card_lines,
            terminal_width: self.width,
            terminal_height: self.height,
            spinner_frame: self.spinner_frame,
            theme: Some(&self.theme),
        });

        super::paint::render_live_diff(&mut self.backend, self.rendered.as_ref(), &completed_layout)?;

        let rendered_transcript = format!("\n{block}");
        self.cache.push(
            super::cache::target_slot(&item, tools_expanded, hide_thinking),
            &rendered_transcript,
        );
        self.transcript.push(item);

        let formatted = super::ansi::terminal_newlines(&crate::ui::interactive::region::paint_region(
            &rendered_transcript,
            &self.theme,
            self.width,
        ));
        self.output.update(&formatted);
        if self.output.is_open() {
            self.output.update("\n");
        }

        self.state.set_active_tool(None);
        self.state.footer_mut().running_tool = None;

        self.rendered = Some(self.current_layout());
        self.backend.flush()
    }
}
