use std::time::Instant;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::ui::interactive::Activity;
use crate::ui::interactive::pacer::{Pacer, Unit};

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
    pub arguments: serde_json::Value,
    /// The state of the file the tool changes, taken before it runs.
    pub file_change: Option<rho_harness_core::presentation::FileChange>,
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
    Recap(String),
    RunningTool(Option<String>),
    SystemMessage(Option<String>),
    Interaction {
        prompt: InteractionPrompt,
        responder: InteractionResponder,
    },
    DismissInteraction,
    A2ui {
        message: rho_harness_core::a2ui::A2uiMessage,
    },
    A2uiModalRequest {
        request: rho_harness_core::a2ui::A2uiModalRequest,
        response_tx: oneshot::Sender<rho_harness_core::a2ui::A2uiModalResult>,
    },
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
    tool_chunks: Pacer,
    stream_text: Pacer,
    tool_end: bool,
    system_message: Option<Option<String>>,
    transcript_items: Vec<crate::ui::interactive::TranscriptItem>,
    trailing_recap: Option<String>,
    a2ui_messages: Vec<rho_harness_core::a2ui::A2uiMessage>,
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
    pub trailing_recap: Option<String>,
    pub a2ui_messages: Vec<rho_harness_core::a2ui::A2uiMessage>,
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
            tool_chunks: Pacer::new(Unit::Line),
            stream_text: Pacer::new(Unit::Word),
            tool_end: false,
            system_message: None,
            transcript_items: Vec::new(),
            trailing_recap: None,
            a2ui_messages: Vec::new(),
            max_text_bytes: max_text_bytes.max(1),
        }
    }

    fn push_output(&mut self, output: OutputEvent, now: Instant) {
        self.total_output_bytes += match &output {
            OutputEvent::Text(text) | OutputEvent::StreamText(text) => text.len(),
        };
        match output {
            OutputEvent::Text(text) => {
                self.release_stream_text(Pacer::take_all);
                self.append_output(OutputEvent::Text(text));
            }
            OutputEvent::StreamText(text) => self.stream_text.push(&text, now),
        }
    }

    fn release_stream_text(&mut self, release: impl FnOnce(&mut Pacer) -> String) {
        let text = release(&mut self.stream_text);
        if !text.is_empty() {
            self.append_output(OutputEvent::StreamText(text));
        }
    }

    fn append_output(&mut self, output: OutputEvent) {
        match (self.outputs.last_mut(), output) {
            (Some(OutputEvent::Text(existing)), OutputEvent::Text(text))
            | (Some(OutputEvent::StreamText(existing)), OutputEvent::StreamText(text)) => existing.push_str(&text),
            (_, output) => self.outputs.push(output),
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

    fn push_flushing(&mut self, event: UiEvent, now: Instant) -> BatchDecision {
        match event {
            UiEvent::ToolStart(r) => {
                self.tool_start = Some(r);
                self.tool_chunks.clear();
                Self::flushes()
            }
            UiEvent::ToolChunk { chunk } => {
                self.tool_chunks.push(&chunk, now);
                Self::flushes()
            }
            UiEvent::Transcript(item) => {
                self.transcript_items.push(item);
                Self::flushes()
            }
            UiEvent::Recap(text) => {
                self.trailing_recap = Some(text);
                Self::flushes()
            }
            UiEvent::SystemMessage(m) => {
                self.system_message = Some(m);
                Self::flushes()
            }
            UiEvent::A2ui { message } => {
                self.a2ui_messages.push(message);
                Self::flushes()
            }
            _ => BatchDecision::Pending,
        }
    }

    pub fn push(&mut self, event: UiEvent) -> BatchDecision {
        self.push_at(event, Instant::now())
    }

    pub fn push_at(&mut self, event: UiEvent, now: Instant) -> BatchDecision {
        match event {
            UiEvent::Output(output) => {
                let (has_newline, len) = match &output {
                    OutputEvent::Text(text) | OutputEvent::StreamText(text) => (text.contains('\n'), text.len()),
                };
                if len > 0 {
                    self.push_output(output, now);
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
            event @ UiEvent::Interaction { .. } | event @ UiEvent::A2uiModalRequest { .. } => {
                BatchDecision::Barrier(FlushBarrier::Interaction, event)
            }
            UiEvent::DismissInteraction => Self::flushes(),
            other => self.push_flushing(other, now),
        }
    }

    pub fn drain(&mut self) -> PendingUiDrain {
        self.drain_at(Instant::now())
    }

    /// Everything pending, except that streamed text and tool output behind a burst are released a little
    /// per call, unless something else in the batch (a tool end, a commit) needs them shown first.
    pub fn drain_at(&mut self, now: Instant) -> PendingUiDrain {
        self.total_output_bytes = 0;
        let ordered_after = self.tool_start.is_some()
            || self.tool_end
            || !self.transcript_items.is_empty()
            || self.trailing_recap.is_some();
        if ordered_after {
            self.release_stream_text(Pacer::take_all);
        } else {
            self.release_stream_text(|pacer| pacer.release(now));
        }
        let chunk = if ordered_after || !self.outputs.is_empty() {
            self.tool_chunks.take_all()
        } else {
            self.tool_chunks.release(now)
        };
        PendingUiDrain {
            outputs: std::mem::take(&mut self.outputs),
            activity: self.activity.take(),
            running_tool: self.running_tool.take(),
            tool_start: self.tool_start.take(),
            tool_chunks: if chunk.is_empty() { Vec::new() } else { vec![chunk] },
            tool_end: std::mem::replace(&mut self.tool_end, false),
            system_message: self.system_message.take(),
            transcript_items: std::mem::take(&mut self.transcript_items),
            trailing_recap: self.trailing_recap.take(),
            a2ui_messages: std::mem::take(&mut self.a2ui_messages),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.outputs.is_empty()
            && self.stream_text.is_empty()
            && self.activity.is_none()
            && self.running_tool.is_none()
            && self.tool_start.is_none()
            && self.tool_chunks.is_empty()
            && !self.tool_end
            && self.system_message.is_none()
            && self.transcript_items.is_empty()
            && self.trailing_recap.is_none()
            && self.a2ui_messages.is_empty()
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

    pub fn push_recap(&self, text: String) -> Result<(), UiPortError> {
        self.sender.send(UiEvent::Recap(text)).map_err(|_| UiPortError::Closed)
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

    pub fn dismiss_interaction(&self) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::DismissInteraction)
            .map_err(|_| UiPortError::Closed)
    }

    pub fn emit_a2ui(&self, message: rho_harness_core::a2ui::A2uiMessage) -> Result<(), UiPortError> {
        self.sender
            .send(UiEvent::A2ui { message })
            .map_err(|_| UiPortError::Closed)
    }

    pub async fn request_a2ui_modal(
        &self,
        request: rho_harness_core::a2ui::A2uiModalRequest,
    ) -> Result<rho_harness_core::a2ui::A2uiModalResult, UiPortError> {
        let (response_tx, receiver) = oneshot::channel();
        self.sender
            .send(UiEvent::A2uiModalRequest { request, response_tx })
            .map_err(|_| UiPortError::Closed)?;
        receiver.await.map_err(|_| UiPortError::Closed)
    }
}
