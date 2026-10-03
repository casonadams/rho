use rho_harness_core::error::AppError;
use rho_harness_core::model::ChatMessage;
use rig::streaming::{Item, Part, StreamEvent};
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub enum DisplayEvent {
    Text(String),
    Reasoning(String),
    ToolCall { name: String, arguments: serde_json::Value },
}

pub fn display_events(item: Item<StreamEvent>, reasoning_parts: &mut HashSet<Part>) -> Vec<DisplayEvent> {
    let Item::Event(event) = item else {
        return Vec::new();
    };
    match event {
        StreamEvent::Text { text, .. } => vec![DisplayEvent::Text(text)],
        StreamEvent::Reasoning { part, text } => {
            reasoning_parts.insert(part);
            vec![DisplayEvent::Reasoning(text)]
        }
        StreamEvent::End { part, content } => match content {
            rig::message::AssistantContent::ToolCall(tool_call) => vec![DisplayEvent::ToolCall {
                name: tool_call.function.name.to_string(),
                arguments: tool_call.function.arguments,
            }],
            rig::message::AssistantContent::Reasoning(sealed) if !reasoning_parts.contains(&part) => {
                if let Some(reasoning) = sealed.open(sealed.issuer()) {
                    let text = reasoning.display_text();
                    if !text.is_empty() {
                        vec![DisplayEvent::Reasoning(text)]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        },
        StreamEvent::Start { .. } | StreamEvent::Arguments { .. } => Vec::new(),
    }
}

pub fn budget_history(error: &rig::agent::StreamingError) -> Option<(usize, Vec<rig::message::Message>)> {
    let rig::agent::StreamingError::Prompt(error) = error else {
        return None;
    };
    let rig::completion::PromptError::MaxTurnsError {
        max_turns,
        chat_history,
        ..
    } = error
    else {
        return None;
    };
    Some((*max_turns, chat_history.clone()))
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

pub fn streaming_error_provider_headers(error: &rig::agent::StreamingError) -> Option<&reqwest::header::HeaderMap> {
    match error {
        rig::agent::StreamingError::Completion(err) => err.provider_response_headers(),
        rig::agent::StreamingError::Prompt(err) => err.provider_response_headers(),
        rig::agent::StreamingError::Report(err) => err.provider_response_headers(),
    }
}

pub fn streaming_error_provider_status(error: &rig::agent::StreamingError) -> Option<reqwest::StatusCode> {
    match error {
        rig::agent::StreamingError::Completion(err) => err.provider_response_status(),
        rig::agent::StreamingError::Prompt(err) => err.provider_response_status(),
        rig::agent::StreamingError::Report(err) => err.provider_response_status(),
    }
}

pub fn streaming_error_provider_request_id(error: &rig::agent::StreamingError) -> Option<&str> {
    match error {
        rig::agent::StreamingError::Completion(err) => err.provider_request_id(),
        rig::agent::StreamingError::Prompt(err) => err.provider_request_id(),
        rig::agent::StreamingError::Report(err) => err.provider_request_id(),
    }
}

pub fn extract_retry_after(error: &rig::agent::StreamingError) -> Option<std::time::Duration> {
    let status = streaming_error_provider_status(error)?;
    if status != reqwest::StatusCode::TOO_MANY_REQUESTS && status != reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return None;
    }
    if let Some(headers) = streaming_error_provider_headers(error)
        && let Some(val) = headers.get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok())
    {
        let val = val.trim();
        if let Ok(secs) = val.parse::<u64>() {
            return Some(std::time::Duration::from_secs(secs));
        }
        if let Ok(date) = chrono::DateTime::parse_from_rfc2822(val) {
            let now = chrono::Utc::now();
            let dur = (date.with_timezone(&chrono::Utc) - now)
                .to_std()
                .unwrap_or(std::time::Duration::ZERO);
            return Some(dur);
        }
    }
    Some(std::time::Duration::from_secs(1))
}

pub fn is_transient_network_error(error: &rig::agent::StreamingError) -> bool {
    if let Some(status) = streaming_error_provider_status(error) {
        return matches!(status.as_u16(), 408 | 502 | 503 | 504);
    }
    let msg = match error {
        rig::agent::StreamingError::Completion(err) => err.to_string(),
        rig::agent::StreamingError::Prompt(err) => err.to_string(),
        rig::agent::StreamingError::Report(err) => err.to_string(),
    };
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

pub fn map_streaming_error(error: rig::agent::StreamingError) -> AppError {
    match error {
        rig::agent::StreamingError::Completion(error) => map_provider_error(error),
        rig::agent::StreamingError::Prompt(error) => map_prompt_error(error),
        rig::agent::StreamingError::Report(error) => map_error_report(error),
    }
}

pub fn map_completion_error(error: rig::error::ProviderError) -> AppError {
    map_provider_error(error)
}

pub fn map_prompt_error(error: rig::completion::PromptError) -> AppError {
    match error {
        rig::completion::PromptError::MaxTurnsError { max_turns, .. } => AppError::ModelBudgetExhausted { max_turns },
        rig::completion::PromptError::PromptCancelled { reason, .. } => {
            AppError::Cancelled(super::helpers::redact_text(&reason))
        }
        rig::completion::PromptError::UnknownToolCall { tool_name, .. } => {
            AppError::InvalidToolCall(tool_name.to_string())
        }
        rig::completion::PromptError::CompletionError(error) => map_provider_error(error),
        rig::completion::PromptError::MemoryError(_) => AppError::Session("Conversation memory failed".to_string()),
        rig::completion::PromptError::Report(r) => map_error_report(r),
    }
}

fn map_error_report(report: rig::error::ErrorReport) -> AppError {
    let req_suffix = report
        .provider_request_id()
        .map(sanitize_request_id)
        .filter(|id| !id.is_empty())
        .map(|id| format!(" (Request ID: {id})"))
        .unwrap_or_default();
    let status = report.provider_response_status().map(|s| s.as_u16());
    let err_msg = super::helpers::redact_text(&report.to_string());
    match status {
        Some(code @ (401 | 403)) => AppError::Auth(format!(
            "Model provider authentication failed (HTTP {code}){req_suffix}"
        )),
        Some(408 | 429 | 500..=599) => {
            AppError::Network(format!("Model provider request could not be completed{req_suffix}"))
        }
        Some(status) => AppError::Provider(format!(
            "Model provider request failed (HTTP {status}): {err_msg}{req_suffix}"
        )),
        None => AppError::Network(format!("Model provider request failed: {err_msg}{req_suffix}")),
    }
}

pub fn map_provider_error(error: rig::error::ProviderError) -> AppError {
    if matches!(
        &error,
        rig::error::ProviderError::Response(message) if message.contains("ContentFilter")
    ) {
        return AppError::ContentFiltered;
    }
    let req_suffix = error
        .provider_request_id()
        .map(sanitize_request_id)
        .filter(|id| !id.is_empty())
        .map(|id| format!(" (Request ID: {id})"))
        .unwrap_or_default();
    let status = error.provider_response_status().map(|s| s.as_u16());
    let err_msg = super::helpers::redact_text(&error.to_string());
    match status {
        Some(code @ (401 | 403)) => AppError::Auth(format!(
            "Model provider authentication failed (HTTP {code}){req_suffix}"
        )),
        Some(408 | 429 | 500..=599) => {
            AppError::Network(format!("Model provider request could not be completed{req_suffix}"))
        }
        Some(status) => AppError::Provider(format!(
            "Model provider request failed (HTTP {status}): {err_msg}{req_suffix}"
        )),
        None => AppError::Network(format!("Model provider request failed: {err_msg}{req_suffix}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use rig::ProviderResponseError;
    use rig::error::ProviderError;

    #[test]
    fn test_sanitize_request_id() {
        assert_eq!(sanitize_request_id("req_12345"), "req_12345");
        assert_eq!(sanitize_request_id("chatcmpl-abc\n"), "chatcmpl-abc");
        assert_eq!(
            sanitize_request_id("sk-secret-key"),
            "sensitive upstream detail redacted"
        );
        assert_eq!(
            sanitize_request_id("Bearer my-token"),
            "sensitive upstream detail redacted"
        );
    }

    #[test]
    fn test_map_completion_error_includes_request_id() {
        let err = ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::INTERNAL_SERVER_ERROR, "upstream fault")
                .with_provider_request_id(Some("req-xyz-99".to_string())),
        );
        let app_err = map_completion_error(err);
        let msg = app_err.to_string();
        assert!(
            msg.contains("(Request ID: req-xyz-99)"),
            "expected request id in: {msg}"
        );

        let err_400 = ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::BAD_REQUEST, "invalid prompt")
                .with_provider_request_id(Some("anthropic-req-123".to_string())),
        );
        let app_err_400 = map_completion_error(err_400);
        let msg_400 = app_err_400.to_string();
        assert!(
            msg_400.contains("(Request ID: anthropic-req-123)"),
            "expected request id in: {msg_400}"
        );
    }

    #[test]
    fn test_extract_retry_after() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_static("3"),
        );

        let err_429 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::TOO_MANY_REQUESTS, "slow down").with_headers(Some(headers)),
        ));
        let duration = extract_retry_after(&err_429);
        assert_eq!(duration, Some(std::time::Duration::from_secs(3)));

        let err_503 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::SERVICE_UNAVAILABLE, "busy"),
        ));
        assert_eq!(extract_retry_after(&err_503), Some(std::time::Duration::from_secs(1)));

        let err_400 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::BAD_REQUEST, "bad input"),
        ));
        assert_eq!(extract_retry_after(&err_400), None);
    }

    #[test]
    fn test_is_transient_network_error() {
        let status_502 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::BAD_GATEWAY, "bad gateway"),
        ));
        assert!(is_transient_network_error(&status_502));

        let status_503 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::SERVICE_UNAVAILABLE, "busy"),
        ));
        assert!(is_transient_network_error(&status_503));

        let status_504 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::GATEWAY_TIMEOUT, "gateway timeout"),
        ));
        assert!(is_transient_network_error(&status_504));

        let status_400 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::BAD_REQUEST, "invalid input"),
        ));
        assert!(!is_transient_network_error(&status_400));

        let status_401 = rig::agent::StreamingError::Completion(rig::error::ProviderError::ProviderResponse(
            ProviderResponseError::new(StatusCode::UNAUTHORIZED, "unauthorized"),
        ));
        assert!(!is_transient_network_error(&status_401));

        let transport_err = rig::agent::StreamingError::Completion(rig::error::ProviderError::Provider(
            "error sending request for url (https://api.anthropic.com/v1/messages)".to_string(),
        ));
        assert!(is_transient_network_error(&transport_err));

        let conn_reset = rig::agent::StreamingError::Completion(rig::error::ProviderError::Provider(
            "connection reset by peer".to_string(),
        ));
        assert!(is_transient_network_error(&conn_reset));

        let stream_err = rig::agent::StreamingError::Completion(rig::error::ProviderError::Provider(
            "Claude stream failed: broken pipe".to_string(),
        ));
        assert!(is_transient_network_error(&stream_err));

        let quota_err = rig::agent::StreamingError::Completion(rig::error::ProviderError::Provider(
            "Antigravity request failed: Individual quota reached for gemini-2.5-pro".to_string(),
        ));
        assert!(!is_transient_network_error(&quota_err));
    }
}
