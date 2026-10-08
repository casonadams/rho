use rig::completion::PromptError;

pub fn is_context_overflow_error(error: &PromptError) -> bool {
    is_context_overflow_message(&error.to_string())
}

pub fn is_context_overflow_message(msg: &str) -> bool {
    let haystack = msg.to_lowercase();
    const SIGNALS: &[&str] = &[
        "context_length_exceeded",
        "context length exceeded",
        "context window exceeded",
        "context window is full",
        "context_window_exceeded",
        "maximum context length",
        "exceeds the context window",
        "exceeds maximum context",
        "prompt is too long",
        "prompt exceeds",
        "prompt_length_exceeded",
        "input token count exceeds",
        "total input tokens exceed",
        "token limit exceeded",
        "too many tokens",
        "request payload size exceeds the limit",
        "resourceexhausted",
        "resource_exhausted",
    ];
    SIGNALS.iter().any(|needle| haystack.contains(needle))
}
