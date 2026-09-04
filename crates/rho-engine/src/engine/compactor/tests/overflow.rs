use rig::agent::StreamingError;
use rig::completion::{CompletionError, PromptError};

use crate::engine::compactor::{is_context_overflow_error, is_context_overflow_message};

#[test]
fn test_is_context_overflow_message_patterns() {
    assert!(is_context_overflow_message(
        "InvalidRequestError: This model's maximum context length is 128000 tokens."
    ));
    assert!(is_context_overflow_message(
        "error: prompt is too long: 205000 tokens > 200000 maximum tokens"
    ));
    assert!(is_context_overflow_message(
        "ResourceExhausted: input token count exceeds limit"
    ));
    assert!(is_context_overflow_message("context_length_exceeded"));
    assert!(is_context_overflow_message("context window exceeded"));
    assert!(is_context_overflow_message(
        "Request payload size exceeds the limit: 1048576 bytes"
    ));
    assert!(is_context_overflow_message(
        "exceeds the context window of 128000 tokens"
    ));

    assert!(!is_context_overflow_message("Connection reset by peer"));
    assert!(!is_context_overflow_message("Unauthorized 401"));
    assert!(!is_context_overflow_message("Internal server error 500"));
    assert!(!is_context_overflow_message("Rate limit exceeded 429"));
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
