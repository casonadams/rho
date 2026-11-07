use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::ui::interactive::Activity;

pub use rho_harness_core::presentation::{
    InteractionInput, InteractionOption, InteractionPrompt, InteractionResponse, OptionLayout,
};

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    Text(String),
    StreamText(String),
}

#[derive(Debug, Error)]
pub enum UiPortError {
    #[error("interactive UI controller stopped")]
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolStartRequest {
    pub name: String,
    pub args_summary: String,
    pub preview: Option<String>,
}

pub struct InteractionResponder {
    pub(crate) responder: oneshot::Sender<InteractionResponse>,
}

impl InteractionResponder {
    pub fn respond(self, response: InteractionResponse) -> Result<(), InteractionResponse> {
        self.responder.send(response)
    }
}

pub enum UiEvent {
    Output(OutputEvent),
    Activity(crate::ui::interactive::Activity),
    ToolStart(ToolStartRequest),
    ToolChunk {
        chunk: String,
    },
    ToolEnd,
    Transcript(crate::ui::interactive::TranscriptItem),
    RunningTool(Option<String>),
    SystemMessage(Option<String>),
    Interaction {
        prompt: InteractionPrompt,
        responder: InteractionResponder,
    },
    DismissInteraction,
}

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
            event @ UiEvent::Interaction { .. } => BatchDecision::Barrier(FlushBarrier::Interaction, event),
            UiEvent::DismissInteraction => Self::flushes(),
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
            && self.system_message.is_none()
            && self.transcript_items.is_empty()
    }
}

#[derive(Clone)]
pub struct InteractiveUi {
    sender: mpsc::UnboundedSender<UiEvent>,
}

impl InteractiveUi {
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<UiEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (Self { sender }, receiver)
    }

    pub fn output(&self, event: OutputEvent) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::Output(event))
            .map_err(|_| UiPortError::Closed)
    }

    pub fn set_activity(&self, activity: Activity) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::Activity(activity))
            .map_err(|_| UiPortError::Closed)
    }

    pub fn set_running_tool(&self, command: Option<String>) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::RunningTool(command))
            .map_err(|_| UiPortError::Closed)
    }

    pub fn set_system_message(&self, message: Option<String>) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::SystemMessage(message))
            .map_err(|_| UiPortError::Closed)
    }

    pub fn tool_start(&self, request: ToolStartRequest) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::ToolStart(request))
            .map_err(|_| UiPortError::Closed)
    }

    pub fn tool_chunk(&self, chunk: String) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::ToolChunk { chunk })
            .map_err(|_| UiPortError::Closed)
    }

    pub fn tool_end(&self) -> Result<(), UiPortError> {
        self.sender.send(UiEvent::ToolEnd).map_err(|_| UiPortError::Closed)
    }

    pub fn push_transcript(&self, item: crate::ui::interactive::TranscriptItem) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::Transcript(item))
            .map_err(|_| UiPortError::Closed)
    }

    pub async fn request(&self, prompt: InteractionPrompt) -> Result<InteractionResponse, UiPortError> {
        let (responder, receiver) = oneshot::channel();
        self.sender
            .send(UiEvent::Interaction {
                prompt,
                responder: InteractionResponder { responder },
            })
            .map_err(|_| UiPortError::Closed)?;
        receiver.await.map_err(|_| UiPortError::Closed)
    }

    pub async fn interact(&self, prompt: InteractionPrompt) -> Result<InteractionResponse, UiPortError> {
        self.request(prompt).await
    }

    pub fn dismiss_interaction(&self) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::DismissInteraction)
            .map_err(|_| UiPortError::Closed)
    }
}
