pub fn is_context_overflow_error(error: &(impl std::fmt::Display + ?Sized)) -> bool {
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
    ];
    SIGNALS.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::is_context_overflow_message;

    #[test]
    fn test_quota_resource_exhausted_is_not_context_overflow() {
        assert!(!is_context_overflow_message(
            "RESOURCE_EXHAUSTED: Quota exceeded for quota metric 'generate_content_requests'"
        ));
    }

    #[test]
    fn test_provider_prompt_too_long_is_context_overflow() {
        assert!(is_context_overflow_message(
            "prompt is too long: 201234 tokens > 200000 maximum"
        ));
    }
}
