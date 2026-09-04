use rig::agent::StreamingError;

pub fn is_context_overflow_error(error: &StreamingError) -> bool {
    is_context_overflow_message(&error.to_string())
}

pub fn is_context_overflow_message(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("context_length_exceeded")
        || lower.contains("context length exceeded")
        || lower.contains("context window exceeded")
        || lower.contains("context window is full")
        || lower.contains("context_window_exceeded")
        || lower.contains("maximum context length")
        || lower.contains("exceeds the context window")
        || lower.contains("exceeds maximum context")
        || lower.contains("prompt is too long")
        || lower.contains("prompt exceeds")
        || lower.contains("prompt_length_exceeded")
        || lower.contains("input token count exceeds")
        || lower.contains("total input tokens exceed")
        || lower.contains("token limit exceeded")
        || lower.contains("too many tokens")
        || lower.contains("request payload size exceeds the limit")
        || lower.contains("resourceexhausted")
        || lower.contains("resource_exhausted")
}
