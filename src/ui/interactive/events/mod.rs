pub mod batch;
#[cfg(test)]
mod tests;
pub mod types;

pub use batch::{BatchDecision, FlushBarrier, PendingUiBatch, PendingUiDrain};
pub use types::{
    InteractionInput, InteractionOption, InteractionPrompt, InteractionResponder, InteractionResponse, OutputEvent,
    ToolStartRequest, UiEvent, UiPortError,
};

use crate::ui::interactive::Activity;
use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct InteractiveUi {
    transport: Arc<Transport>,
}

enum Transport {
    Channel(mpsc::UnboundedSender<UiEvent>),
    Writer(Mutex<Box<dyn Write + Send>>),
}

impl InteractiveUi {
    pub fn channel() -> (Self, mpsc::UnboundedReceiver<UiEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            Self {
                transport: Arc::new(Transport::Channel(sender)),
            },
            receiver,
        )
    }

    pub fn writer(writer: impl Write + Send + 'static) -> Self {
        Self {
            transport: Arc::new(Transport::Writer(Mutex::new(Box::new(writer)))),
        }
    }

    pub fn output(&self, event: OutputEvent) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender.send(UiEvent::Output(event)).map_err(|_| UiPortError::Closed),
            Transport::Writer(writer) => {
                let mut writer = writer
                    .lock()
                    .map_err(|_| io::Error::other("interactive UI writer lock poisoned"))?;
                match event {
                    OutputEvent::Text(text) => writer.write_all(text.as_bytes())?,
                }
                writer.flush()?;
                Ok(())
            }
        }
    }

    pub fn set_activity(&self, activity: Activity) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender
                .send(UiEvent::Activity(activity))
                .map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn set_running_tool(&self, command: Option<String>) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender
                .send(UiEvent::RunningTool(command))
                .map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn set_extra_status(&self, status: Option<String>) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender
                .send(UiEvent::ExtraStatus(status))
                .map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn tool_start(&self, request: ToolStartRequest) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender
                .send(UiEvent::ToolStart(request))
                .map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn tool_chunk(&self, chunk: String) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender
                .send(UiEvent::ToolChunk { chunk })
                .map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn tool_end(&self) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender.send(UiEvent::ToolEnd).map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub fn push_transcript(&self, item: crate::ui::interactive::TranscriptItem) -> Result<(), UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => sender.send(UiEvent::Transcript(item)).map_err(|_| UiPortError::Closed),
            Transport::Writer(_) => Ok(()),
        }
    }

    pub async fn request(&self, prompt: InteractionPrompt) -> Result<InteractionResponse, UiPortError> {
        match self.transport.as_ref() {
            Transport::Channel(sender) => {
                let (responder, receiver) = oneshot::channel();
                sender
                    .send(UiEvent::Interaction {
                        prompt,
                        responder: InteractionResponder { responder },
                    })
                    .map_err(|_| UiPortError::Closed)?;
                receiver.await.map_err(|_| UiPortError::Closed)
            }
            Transport::Writer(_) => Err(UiPortError::Unavailable),
        }
    }

    pub async fn interact(&self, prompt: InteractionPrompt) -> Result<InteractionResponse, UiPortError> {
        self.request(prompt).await
    }
}
