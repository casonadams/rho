use rig::agent::StreamingError;
use rig::completion::{CompletionError, PromptError};

use crate::engine::compactor::{is_context_overflow_error, is_context_overflow_message};

#[test]
fn test_is_context_overflow_message_patterns() {
    let overflow_messages = [
        "InvalidRequestError: This model's maximum context length is 128000 tokens.",
        "error: prompt is too long: 205000 tokens > 200000 maximum tokens",
        "ResourceExhausted: input token count exceeds limit",
        "context_length_exceeded",
        "context window exceeded",
        "Request payload size exceeds the limit: 1048576 bytes",
        "exceeds the context window of 128000 tokens",
    ];
    for msg in overflow_messages {
        assert!(is_context_overflow_message(msg));
    }
    for msg in [
        "Connection reset by peer",
        "Unauthorized 401",
        "Internal server error 500",
        "Rate limit exceeded 429",
    ] {
        assert!(!is_context_overflow_message(msg));
    }
}

#[test]
fn test_is_context_overflow_streaming_error() {
    let completion_err = StreamingError::Completion(CompletionError::ResponseError(
        "prompt is too long: 210000 tokens".to_string(),
    ));
    assert!(is_context_overflow_error(&completion_err));

    let prompt_err = StreamingError::Prompt(Box::new(PromptError::CompletionError(CompletionError::ResponseError(
        "context_length_exceeded".to_string(),
    ))));
    assert!(is_context_overflow_error(&prompt_err));

    let unrelated = StreamingError::Completion(CompletionError::ResponseError(
        "Model overloaded, try again later".to_string(),
    ));
    assert!(!is_context_overflow_error(&unrelated));
}
