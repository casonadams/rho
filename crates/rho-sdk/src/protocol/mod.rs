pub mod envelope;
pub mod message;
#[cfg(test)]
mod tests;
pub mod validator;

pub use envelope::{
    Envelope, ErrorCode, MAX_PROTOCOL_ERROR_BYTES, MAX_PROTOCOL_ERROR_MESSAGE_BYTES, MAX_PROTOCOL_LINE_BYTES,
    MAX_PROTOCOL_MESSAGE_BYTES, MAX_PROTOCOL_RESULT_BYTES, RequestId, StructuredError, decode_line, encode_line,
};
pub use message::{InvocationRequest, ProtocolMessage, StreamEvent, TerminalResult};
pub use validator::{ProtocolViolation, ResponseSequenceValidator};
