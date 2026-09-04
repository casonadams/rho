//! Structured presentation output sinks and headless presenter.

use super::types::{SessionStatus, ToolLine, UiEnvelope, UiEvent, WelcomeDisplay};
use crate::presentation::activity::{ActivityToken, activity_token};
use crate::presentation::presenter::Presenter;
use crate::presentation::stream::{ToolStreamPort, ToolStreamSink};
use async_trait::async_trait;
use serde_json::Value;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// A thread-safe sink for presentation events.
pub trait StructuredOutputSink: Send + Sync {
    fn emit(&self, event: UiEvent);
    fn flush(&self);
}

/// Sink emitting line-delimited JSON envelopes directly to stdout.
#[derive(Default)]
pub struct StdoutNdjsonSink;

impl StructuredOutputSink for StdoutNdjsonSink {
    fn emit(&self, event: UiEvent) {
        let envelope = UiEnvelope::new(event);
        if let Ok(line) = serde_json::to_string(&envelope) {
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }

    fn flush(&self) {
        let mut out = std::io::stdout().lock();
        let _ = out.flush();
    }
}

/// In-memory sink recording all observed events in chronological order.
#[derive(Default, Clone)]
pub struct RecordingSink {
    events: Arc<Mutex<Vec<UiEvent>>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn events(&self) -> Vec<UiEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }
}

impl StructuredOutputSink for RecordingSink {
    fn emit(&self, event: UiEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn flush(&self) {}
}

/// A presenter that emits structured `UiEvent`s to a `StructuredOutputSink`.
pub struct StructuredPresenter {
    sink: Arc<dyn StructuredOutputSink>,
}

impl StructuredPresenter {
    pub fn new(sink: Arc<dyn StructuredOutputSink>) -> Self {
        Self { sink }
    }

    pub fn stdout() -> Self {
        Self::new(Arc::new(StdoutNdjsonSink))
    }

    pub fn recording(sink: RecordingSink) -> Self {
        Self::new(Arc::new(sink))
    }

    pub fn sink(&self) -> &Arc<dyn StructuredOutputSink> {
        &self.sink
    }
}

struct StructuredStreamSink {
    sink: Arc<dyn StructuredOutputSink>,
}

impl ToolStreamSink for StructuredStreamSink {
    fn tool_chunk(&self, chunk: String) {
        self.sink.emit(UiEvent::ToolChunk {
            name: String::new(),
            chunk,
        });
    }
}

#[async_trait]
impl Presenter for StructuredPresenter {
    fn write_output(&self, text: &str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            self.sink.emit(UiEvent::Notice {
                text: trimmed.to_string(),
            });
        }
    }

    fn print_welcome(&self, display: &WelcomeDisplay) {
        self.sink.emit(UiEvent::Welcome {
            display: display.clone(),
        });
    }

    fn print_session_status(&self, display: &SessionStatus) {
        self.sink.emit(UiEvent::SessionStatus {
            display: display.clone(),
        });
    }

    fn print_notice(&self, text: &str) {
        self.sink.emit(UiEvent::Notice { text: text.to_string() });
    }

    fn print_user_block(&self, input: &str) {
        self.sink.emit(UiEvent::UserBlock {
            input: input.to_string(),
        });
    }

    fn print_token(&self, token: &str) {
        self.sink.emit(UiEvent::Token {
            token: token.to_string(),
        });
    }

    fn print_thinking_token(&self, token: &str) {
        self.sink.emit(UiEvent::ThinkingToken {
            token: token.to_string(),
        });
    }

    fn finish_tool_line(&self, line: ToolLine) {
        self.sink.emit(UiEvent::ToolFinished { line });
    }

    fn flush(&self) {
        self.sink.flush();
    }

    fn has_interactive_ui(&self) -> bool {
        false
    }

    fn start_spinner(&self, message: &str) -> ActivityToken {
        self.sink.emit(UiEvent::ActivityStarted {
            message: message.to_string(),
        });
        let sink = Arc::clone(&self.sink);
        activity_token(move || {
            sink.emit(UiEvent::ActivityFinished);
        })
    }

    fn start_tool_spinner(&self, name: &str, arguments: &Value) -> ActivityToken {
        self.sink.emit(UiEvent::ToolStarted {
            name: name.to_string(),
            arguments: arguments.clone(),
        });
        ActivityToken::default()
    }

    fn start_tool_run(&self, name: &str, arguments: &Value) {
        self.sink.emit(UiEvent::ToolStarted {
            name: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    fn stream_port(&self) -> ToolStreamPort {
        ToolStreamPort::new(Some(Arc::new(StructuredStreamSink {
            sink: Arc::clone(&self.sink),
        })))
    }

    fn print_turn_started(&self, prompt: &str) {
        self.sink.emit(UiEvent::TurnStarted {
            prompt: prompt.to_string(),
        });
    }

    fn print_turn_completed(&self, status: &str) {
        self.sink.emit(UiEvent::TurnCompleted {
            status: status.to_string(),
        });
    }
}

#[cfg(test)]
mod tests;
