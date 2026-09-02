use std::io;
use thiserror::Error;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputEvent {
    Text(String),
}

pub use rho_harness_core::presentation::{InteractionOption, InteractionPrompt, InteractionResponse};

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
    ExtraStatus(Option<String>),
    Interaction {
        prompt: InteractionPrompt,
        responder: InteractionResponder,
    },
}
