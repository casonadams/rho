use std::io;
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionPrompt {
    pub title: String,
    pub body: String,
    pub options: Vec<InteractionOption>,
    pub initial_selection: usize,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionResponse {
    Selected(usize),
    Custom(String),
    Cancelled,
}

#[derive(Debug, Error)]
pub enum UiPortError {
    #[error("interactive UI is unavailable")]
    Unavailable,
    #[error("interactive UI controller stopped")]
    Closed,
    #[error("interactive UI output failed: {0}")]
    Output(#[from] io::Error),
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
    Interaction {
        prompt: InteractionPrompt,
        responder: InteractionResponder,
    },
}
