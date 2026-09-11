use super::types::{OutputEvent, ToolStartRequest, UiEvent};
use crate::ui::interactive::Activity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushBarrier {
    Frame,
    Newline,
    Size,
    Interaction,
    Completion,
    Error,
    Cancellation,
    Suspension,
}

pub enum BatchDecision {
    Pending,
    Flush(FlushBarrier),
    Barrier(FlushBarrier, UiEvent),
}

#[derive(Debug)]
pub struct PendingUiBatch {
    outputs: Vec<OutputEvent>,
    total_output_bytes: usize,
    activity: Option<Activity>,
    running_tool: Option<Option<String>>,
    tool_start: Option<ToolStartRequest>,
    tool_chunks: Vec<String>,
    tool_end: bool,
    extra_status: Option<Option<String>>,
    system_message: Option<Option<String>>,
    transcript_items: Vec<crate::ui::interactive::TranscriptItem>,
    max_text_bytes: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PendingUiDrain {
    pub outputs: Vec<OutputEvent>,
    pub activity: Option<Activity>,
    pub running_tool: Option<Option<String>>,
    pub tool_start: Option<ToolStartRequest>,
    pub tool_chunks: Vec<String>,
    pub tool_end: bool,
    pub extra_status: Option<Option<String>>,
    pub system_message: Option<Option<String>>,
    pub transcript_items: Vec<crate::ui::interactive::TranscriptItem>,
}

impl PendingUiDrain {
    pub fn text(&self) -> String {
        let mut text = String::new();
        for output in &self.outputs {
            match output {
                OutputEvent::Text(s) | OutputEvent::StreamText(s) => text.push_str(s),
            }
        }
        text
    }
}

impl PendingUiBatch {
    pub fn new(max_text_bytes: usize) -> Self {
        Self {
            outputs: Vec::new(),
            total_output_bytes: 0,
            activity: None,
            running_tool: None,
            tool_start: None,
            tool_chunks: Vec::new(),
            tool_end: false,
            extra_status: None,
            system_message: None,
            transcript_items: Vec::new(),
            max_text_bytes: max_text_bytes.max(1),
        }
    }

    fn push_output(&mut self, output: OutputEvent, len: usize) {
        self.total_output_bytes += len;
        match output {
            OutputEvent::Text(text) => {
                if let Some(OutputEvent::Text(existing)) = self.outputs.last_mut() {
                    existing.push_str(&text);
                } else {
                    self.outputs.push(OutputEvent::Text(text));
                }
            }
            OutputEvent::StreamText(text) => {
                if let Some(OutputEvent::StreamText(existing)) = self.outputs.last_mut() {
                    existing.push_str(&text);
                } else {
                    self.outputs.push(OutputEvent::StreamText(text));
                }
            }
        }
    }

    fn flush_barrier_for_text(&self, has_newline: bool) -> BatchDecision {
        if has_newline {
            BatchDecision::Flush(FlushBarrier::Newline)
        } else if self.total_output_bytes >= self.max_text_bytes {
            BatchDecision::Flush(FlushBarrier::Size)
        } else {
            BatchDecision::Pending
        }
    }

    fn flushes() -> BatchDecision {
        BatchDecision::Flush(FlushBarrier::Newline)
    }

    fn push_flushing(&mut self, event: UiEvent) -> BatchDecision {
        match event {
            UiEvent::ToolStart(r) => {
                self.tool_start = Some(r);
                Self::flushes()
            }
            UiEvent::ToolChunk { chunk } => {
                self.tool_chunks.push(chunk);
                Self::flushes()
            }
            UiEvent::Transcript(item) => {
                self.transcript_items.push(item);
                Self::flushes()
            }
            UiEvent::SystemMessage(m) => {
                self.system_message = Some(m);
                Self::flushes()
            }
            _ => BatchDecision::Pending,
        }
    }

    pub fn push(&mut self, event: UiEvent) -> BatchDecision {
        match event {
            UiEvent::Output(output) => {
                let (has_newline, len) = match &output {
                    OutputEvent::Text(text) | OutputEvent::StreamText(text) => (text.contains('\n'), text.len()),
                };
                if len > 0 {
                    self.push_output(output, len);
                }
                self.flush_barrier_for_text(has_newline)
            }
            UiEvent::Activity(a) => {
                self.activity = Some(a);
                BatchDecision::Pending
            }
            UiEvent::ToolEnd => {
                self.tool_end = true;
                BatchDecision::Pending
            }
            UiEvent::RunningTool(u) => {
                self.running_tool = Some(u);
                BatchDecision::Pending
            }
            UiEvent::ExtraStatus(s) => {
                self.extra_status = Some(s);
                BatchDecision::Pending
            }
            event @ UiEvent::Interaction { .. } => BatchDecision::Barrier(FlushBarrier::Interaction, event),
            other => self.push_flushing(other),
        }
    }

    pub fn drain(&mut self) -> PendingUiDrain {
        self.total_output_bytes = 0;
        PendingUiDrain {
            outputs: std::mem::take(&mut self.outputs),
            activity: self.activity.take(),
            running_tool: self.running_tool.take(),
            tool_start: self.tool_start.take(),
            tool_chunks: std::mem::take(&mut self.tool_chunks),
            tool_end: std::mem::replace(&mut self.tool_end, false),
            extra_status: self.extra_status.take(),
            system_message: self.system_message.take(),
            transcript_items: std::mem::take(&mut self.transcript_items),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
            && self.activity.is_none()
            && self.running_tool.is_none()
            && self.tool_start.is_none()
            && self.tool_chunks.is_empty()
            && !self.tool_end
            && self.extra_status.is_none()
            && self.system_message.is_none()
            && self.transcript_items.is_empty()
    }
}
