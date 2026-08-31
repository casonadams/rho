use super::envelope::{Envelope, RequestId};
use super::message::{ProtocolMessage, StreamEvent};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolViolation {
    #[error("protocol line exceeds the configured size limit")]
    LineTooLarge,
    #[error("protocol message exceeds the configured size limit")]
    MessageTooLarge,
    #[error("protocol result exceeds the configured size limit")]
    ResultTooLarge,
    #[error("structured protocol error exceeds the configured size limit")]
    ErrorTooLarge,
    #[error("protocol input is not valid UTF-8")]
    InvalidUtf8,
    #[error("protocol input must contain exactly one NDJSON object")]
    InvalidFraming,
    #[error("protocol message is malformed")]
    Malformed,
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(u32),
    #[error("request identifier is invalid")]
    InvalidRequestId,
    #[error("response request identifier does not match the active request")]
    CorrelationMismatch,
    #[error("stream event arrived after a terminal response")]
    EventAfterTerminal,
    #[error("response sequence contains more than one terminal response")]
    DuplicateTerminal,
    #[error("response sequence did not contain a terminal response")]
    MissingTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseSequenceValidator {
    request_id: RequestId,
    terminal: bool,
    event_kinds: BTreeSet<&'static str>,
}

impl ResponseSequenceValidator {
    pub fn new(request_id: RequestId) -> Self {
        Self {
            request_id,
            terminal: false,
            event_kinds: BTreeSet::new(),
        }
    }

    pub fn accept(&mut self, envelope: &Envelope) -> Result<(), ProtocolViolation> {
        if envelope.request_id != self.request_id {
            return Err(ProtocolViolation::CorrelationMismatch);
        }
        if self.terminal {
            return if envelope.message.is_terminal() {
                Err(ProtocolViolation::DuplicateTerminal)
            } else {
                Err(ProtocolViolation::EventAfterTerminal)
            };
        }
        match &envelope.message {
            ProtocolMessage::StreamEvent { event } => {
                self.event_kinds.insert(match event {
                    StreamEvent::Provider(_) => "provider",
                    StreamEvent::Progress { .. } => "progress",
                    StreamEvent::CommandOutput { .. } => "command_output",
                });
            }
            message if message.is_terminal() => self.terminal = true,
            _ => return Err(ProtocolViolation::Malformed),
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), ProtocolViolation> {
        if self.terminal {
            Ok(())
        } else {
            Err(ProtocolViolation::MissingTerminal)
        }
    }
}
