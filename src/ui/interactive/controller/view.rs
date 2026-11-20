//! Shows another agent in the terminal in place of the main agent. The main agent's transcript,
//! partial output, and running tool are parked, and output it produces meanwhile is held, so
//! returning replays its transcript and then everything it did while another agent was shown.

use super::TerminalController;
use super::ansi::terminal_newlines;
use super::backend::TerminalBackend;
use super::cache::target_slot;
use crate::ui::interactive::{PendingUiDrain, RunningTool, ToolStartRequest, TranscriptItem};
use std::io;

/// What the shown agent looks like now: its settled transcript, the reply it is still writing,
/// and the tool it is running.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFrame {
    pub items: Vec<TranscriptItem>,
    pub streaming: Option<String>,
    pub running_tool: Option<ToolStartRequest>,
}

pub(crate) struct AgentView {
    agent: u64,
    parked: Parked,
    /// The raw reply text already streamed to the terminal.
    streamed: String,
}

struct Parked {
    transcript: Vec<TranscriptItem>,
    streamed_output: String,
    active_tool: Option<RunningTool>,
    held: Vec<PendingUiDrain>,
}

fn common_prefix(shown: &[TranscriptItem], next: &[TranscriptItem]) -> usize {
    shown.iter().zip(next).take_while(|(a, b)| a == b).count()
}

fn same_tool(running: Option<&RunningTool>, request: Option<&ToolStartRequest>) -> bool {
    match (running, request) {
        (None, None) => true,
        (Some(tool), Some(request)) => tool.name == request.name && tool.args_summary == request.args_summary,
        _ => false,
    }
}

impl<B: TerminalBackend> TerminalController<B> {
    /// The agent shown instead of the main agent, if any.
    pub fn viewed_agent(&self) -> Option<u64> {
        self.view.as_ref().map(|view| view.agent)
    }

    /// Shows agent `agent` as `frame`, titled `title` in the footer. The main agent stays parked
    /// when switching from one agent to another.
    pub fn enter_agent_view(&mut self, agent: u64, title: String, frame: ViewFrame) -> io::Result<()> {
        let parked = match self.view.take() {
            Some(view) => view.parked,
            None => Parked {
                transcript: std::mem::take(&mut self.transcript),
                streamed_output: std::mem::take(&mut self.streamed_output),
                active_tool: self.state.active_tool().cloned(),
                held: Vec::new(),
            },
        };
        self.view = Some(AgentView {
            agent,
            parked,
            streamed: String::new(),
        });
        self.state.footer_mut().viewing = Some(title);
        self.show_frame(frame)
    }

    /// Updates the footer title of the shown agent.
    pub fn set_view_title(&mut self, title: String) -> bool {
        let footer = self.state.footer_mut();
        if footer.viewing.as_deref() == Some(title.as_str()) {
            return false;
        }
        footer.viewing = Some(title);
        true
    }

    /// Replaces the screen with `frame`, replaying it from the top.
    fn show_frame(&mut self, frame: ViewFrame) -> io::Result<()> {
        self.transcript = frame.items;
        self.cache.clear();
        let streamed = frame.streaming.unwrap_or_default();
        self.streamed_output = terminal_newlines(&streamed);
        if let Some(view) = self.view.as_mut() {
            view.streamed = streamed;
        }
        let running = frame
            .running_tool
            .map(|request| RunningTool::new(request.name, request.args_summary, request.preview));
        self.state.footer_mut().running_tool = running.as_ref().map(|tool| tool.name.clone());
        self.state.set_active_tool(running);
        self.replay_transcript()
    }

    /// Brings the shown agent up to `frame`: new items are written below the old ones and the reply
    /// keeps streaming; anything that changed in place is replayed. Returns whether it changed.
    pub fn sync_agent_view(&mut self, frame: ViewFrame) -> io::Result<bool> {
        let Some(view) = self.view.as_ref() else {
            return Ok(false);
        };
        let kept = common_prefix(&self.transcript, &frame.items);
        let rewritten = frame
            .streaming
            .as_deref()
            .is_some_and(|text| !text.starts_with(view.streamed.as_str()));
        if kept < self.transcript.len() || rewritten {
            self.show_frame(frame)?;
            return Ok(true);
        }
        let mut changed = false;
        for item in &frame.items[kept..] {
            self.push_view_item(item.clone())?;
            changed = true;
        }
        changed |= self.stream_view_text(frame.streaming)?;
        changed |= self.set_view_tool(frame.running_tool)?;
        Ok(changed)
    }

    fn push_view_item(&mut self, item: TranscriptItem) -> io::Result<()> {
        let streamed = self.view.as_mut().map(|view| std::mem::take(&mut view.streamed));
        let was_streaming = streamed.is_some_and(|text| !text.is_empty());
        if !matches!(item, TranscriptItem::AssistantText(_) | TranscriptItem::Thinking(_))
            || was_streaming && matches!(item, TranscriptItem::AssistantText(_))
        {
            self.push_transcript_item(item)?;
            return Ok(());
        }
        let needs_prefix_newline = self.output.is_open();
        let rendered = self.render_detached(&item, self.width);
        self.cache.push(
            target_slot(&item, self.state.tools_expanded(), self.state.hide_thinking()),
            &rendered,
        );
        self.transcript.push(item);
        if !rendered.is_empty() {
            self.write_transcript_card(&rendered, needs_prefix_newline)?;
        }
        Ok(())
    }

    fn stream_view_text(&mut self, streaming: Option<String>) -> io::Result<bool> {
        let (Some(text), Some(view)) = (streaming, self.view.as_mut()) else {
            return Ok(false);
        };
        let delta = text[view.streamed.len()..].to_string();
        view.streamed = text;
        if delta.is_empty() {
            return Ok(false);
        }
        self.write_stream_output(&delta)?;
        Ok(true)
    }

    fn set_view_tool(&mut self, request: Option<ToolStartRequest>) -> io::Result<bool> {
        if same_tool(self.state.active_tool(), request.as_ref()) {
            return Ok(false);
        }
        match request {
            Some(request) => self.start_tool(request)?,
            None => self.end_tool()?,
        }
        Ok(true)
    }

    /// Keeps the main agent's output while another agent is shown.
    pub fn hold_output(&mut self, drained: PendingUiDrain) {
        if let Some(view) = self.view.as_mut() {
            view.parked.held.push(drained);
        }
    }

    /// Returns to the main agent: replays its transcript, then applies the output it produced
    /// while another agent was shown.
    pub fn leave_agent_view(&mut self) -> io::Result<()> {
        let Some(view) = self.view.take() else {
            return Ok(());
        };
        let parked = view.parked;
        self.state.footer_mut().viewing = None;
        self.transcript = parked.transcript;
        self.streamed_output = parked.streamed_output;
        self.cache.clear();
        self.state.footer_mut().running_tool = parked.active_tool.as_ref().map(|tool| tool.name.clone());
        self.state.set_active_tool(parked.active_tool);
        self.replay_transcript()?;
        for drained in parked.held {
            self.apply_output(drained)?;
        }
        Ok(())
    }

    /// Applies an agent's tool, output, and transcript updates; returns (state changed, output written).
    pub fn apply_output(&mut self, mut drained: PendingUiDrain) -> io::Result<(bool, bool)> {
        let mut changed = self.apply_tool_updates(&mut drained)?;
        let mut wrote_output = false;
        for output in drained.outputs {
            match output {
                crate::ui::interactive::OutputEvent::Text(text) => self.write_output(&text)?,
                crate::ui::interactive::OutputEvent::StreamText(text) => self.write_stream_output(&text)?,
            }
            wrote_output = true;
        }
        for item in drained.transcript_items {
            wrote_output |= self.push_transcript_item(item)?;
            changed = true;
        }
        Ok((changed, wrote_output))
    }

    fn apply_tool_updates(&mut self, drained: &mut PendingUiDrain) -> io::Result<bool> {
        let mut changed = false;
        if let Some(request) = drained.tool_start.take() {
            self.start_tool(request)?;
            changed = true;
        }
        if !drained.tool_chunks.is_empty() {
            self.append_tool_chunks(drained.tool_chunks.iter().map(String::as_str))?;
        }
        let has_tool_transcript = drained
            .transcript_items
            .iter()
            .any(|item| matches!(item, TranscriptItem::Tool(_)));
        if drained.tool_end && !has_tool_transcript {
            self.end_tool()?;
            changed = true;
        } else if !has_tool_transcript && let Some(running) = drained.running_tool.take() {
            self.state.footer_mut().running_tool = running;
            changed = true;
        }
        Ok(changed)
    }
}
