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
    text: String,
    activity: Option<Activity>,
    running_tool: Option<Option<String>>,
    tool_start: Option<ToolStartRequest>,
    tool_chunks: Vec<String>,
    tool_end: bool,
    extra_status: Option<Option<String>>,
    transcript_items: Vec<crate::ui::interactive::TranscriptItem>,
    max_text_bytes: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PendingUiDrain {
    pub text: String,
    pub activity: Option<Activity>,
    pub running_tool: Option<Option<String>>,
    pub tool_start: Option<ToolStartRequest>,
    pub tool_chunks: Vec<String>,
    pub tool_end: bool,
    pub extra_status: Option<Option<String>>,
    pub transcript_items: Vec<crate::ui::interactive::TranscriptItem>,
}

impl PendingUiBatch {
    pub fn new(max_text_bytes: usize) -> Self {
        Self {
            text: String::new(),
            activity: None,
            running_tool: None,
            tool_start: None,
            tool_chunks: Vec::new(),
            tool_end: false,
            extra_status: None,
            transcript_items: Vec::new(),
            max_text_bytes: max_text_bytes.max(1),
        }
    }

    pub fn push(&mut self, event: UiEvent) -> BatchDecision {
        match event {
            UiEvent::Output(OutputEvent::Text(text)) => {
                let has_newline = text.contains('\n');
                self.text.push_str(&text);
                if has_newline {
                    BatchDecision::Flush(FlushBarrier::Newline)
                } else if self.text.len() >= self.max_text_bytes {
                    BatchDecision::Flush(FlushBarrier::Size)
                } else {
                    BatchDecision::Pending
                }
            }
            UiEvent::Activity(activity) => {
                self.activity = Some(activity);
                BatchDecision::Pending
            }
            UiEvent::ToolStart(request) => {
                self.tool_start = Some(request);
                BatchDecision::Flush(FlushBarrier::Newline)
            }
            UiEvent::ToolChunk { chunk } => {
                self.tool_chunks.push(chunk);
                BatchDecision::Flush(FlushBarrier::Newline)
            }
            UiEvent::ToolEnd => {
                self.tool_end = true;
                BatchDecision::Pending
            }
            UiEvent::Transcript(item) => {
                self.transcript_items.push(item);
                BatchDecision::Flush(FlushBarrier::Newline)
            }
            UiEvent::RunningTool(update) => {
                self.running_tool = Some(update);
                BatchDecision::Pending
            }
            UiEvent::ExtraStatus(status) => {
                self.extra_status = Some(status);
                BatchDecision::Pending
            }
            event @ UiEvent::Interaction { .. } => BatchDecision::Barrier(FlushBarrier::Interaction, event),
        }
    }

    pub fn drain(&mut self) -> PendingUiDrain {
        PendingUiDrain {
            text: std::mem::take(&mut self.text),
            activity: self.activity.take(),
            running_tool: self.running_tool.take(),
            tool_start: self.tool_start.take(),
            tool_chunks: std::mem::take(&mut self.tool_chunks),
            tool_end: std::mem::replace(&mut self.tool_end, false),
            extra_status: self.extra_status.take(),
            transcript_items: std::mem::take(&mut self.transcript_items),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
            && self.activity.is_none()
            && self.running_tool.is_none()
            && self.tool_start.is_none()
            && self.tool_chunks.is_empty()
            && !self.tool_end
            && self.extra_status.is_none()
            && self.transcript_items.is_empty()
    }
}
