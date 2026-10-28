use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;

#[derive(Debug, PartialEq, Eq)]
pub enum DisplayEvent {
    Text(String),
    Reasoning(String),
    ToolCall { name: String, arguments: serde_json::Value },
}

pub fn display_events(event: &super::turn::runner::StreamEvent) -> Vec<DisplayEvent> {
    match event {
        super::turn::runner::StreamEvent::Text(t) => vec![DisplayEvent::Text(t.clone())],
        super::turn::runner::StreamEvent::Reasoning(r) => vec![DisplayEvent::Reasoning(r.clone())],
        super::turn::runner::StreamEvent::Arguments { .. } => Vec::new(),
    }
}

pub fn budget_history(_error: &AppError) -> Option<(usize, Vec<ChatMessage>)> {
    None
}

pub fn checkpoint_messages(
    visible_history: &[ChatMessage],
    full_history: &[ChatMessage],
) -> Result<Vec<ChatMessage>, AppError> {
    full_history
        .strip_prefix(visible_history)
        .filter(|messages| !messages.is_empty())
        .map(<[ChatMessage]>::to_vec)
        .ok_or_else(|| AppError::Session("Budget checkpoint did not match the model-visible history".to_string()))
}

pub fn continuation_history(visible_history: &[ChatMessage], checkpoint: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut history = Vec::with_capacity(visible_history.len() + checkpoint.len());
    history.extend_from_slice(visible_history);
    history.extend_from_slice(checkpoint);
    history
}

pub fn sanitize_request_id(id: &str) -> String {
    let redacted = super::helpers::redact_text(id);
    let lower = redacted.to_ascii_lowercase();
    if lower.contains("sk-") || lower.contains("key-") || lower.contains("token") {
        return "sensitive upstream detail redacted".to_string();
    }
    redacted
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn streaming_error_provider_headers(_error: &AppError) -> Option<&http::HeaderMap> {
    None
}

pub fn streaming_error_provider_status(_error: &AppError) -> Option<http::StatusCode> {
    None
}

pub fn streaming_error_provider_request_id(_error: &AppError) -> Option<&str> {
    None
}

pub fn extract_retry_after(error: &AppError) -> Option<std::time::Duration> {
    let msg = error.to_string();
    if msg.to_lowercase().contains("rate limit") {
        Some(std::time::Duration::from_secs(1))
    } else {
        None
    }
}

pub fn is_transient_network_error(error: &AppError) -> bool {
    let msg = error.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("individual quota reached")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("authentication failed")
    {
        return false;
    }
    const PATTERNS: &[&str] = &[
        "error sending request",
        "connection reset",
        "connection refused",
        "connection closed",
        "broken pipe",
        "timed out",
        "timeout",
        "dns error",
        "failed to lookup address",
        "stream transport failed",
        "stream failed",
        "unexpected eof",
        "channel closed",
        "network error",
        "handshake",
    ];
    PATTERNS.iter().any(|&p| lower.contains(p))
}

pub fn map_streaming_error(error: AppError) -> AppError {
    error
}

pub fn map_prompt_error(error: AppError) -> AppError {
    error
}

pub fn map_completion_error(error: AppError) -> AppError {
    error
}

pub fn is_content_filter_error(error: &AppError) -> bool {
    let msg = error.to_string();
    msg.contains("ContentFilter") || msg.to_lowercase().contains("content_filter")
}
