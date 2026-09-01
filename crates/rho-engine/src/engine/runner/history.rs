use rho_core::error::AppError;
use rig::streaming::StreamedAssistantContent;
use std::collections::HashSet;

#[derive(Debug, PartialEq, Eq)]
pub enum DisplayEvent {
    Text(String),
    Reasoning(String),
    ToolCall { name: String, arguments: serde_json::Value },
}

pub fn display_events(item: StreamedAssistantContent, reasoning_parts: &mut HashSet<String>) -> Vec<DisplayEvent> {
    match item {
        StreamedAssistantContent::Text(text) => vec![DisplayEvent::Text(text.text)],
        StreamedAssistantContent::ReasoningDelta { id, reasoning, .. } => {
            reasoning_parts.insert(id);
            vec![DisplayEvent::Reasoning(reasoning)]
        }
        StreamedAssistantContent::Reasoning { reasoning, id } if !reasoning_parts.contains(&id) => reasoning
            .content
            .into_iter()
            .filter_map(|content| match content {
                rig::message::ReasoningContent::Text { text, .. } | rig::message::ReasoningContent::Summary(text) => {
                    Some(DisplayEvent::Reasoning(text))
                }
                rig::message::ReasoningContent::Encrypted(_) | rig::message::ReasoningContent::Redacted { .. } => None,
            })
            .collect(),
        StreamedAssistantContent::ToolCall { tool_call, .. } => vec![DisplayEvent::ToolCall {
            name: tool_call.function.name,
            arguments: tool_call.function.arguments,
        }],
        StreamedAssistantContent::ToolCallDelta { .. }
        | StreamedAssistantContent::Reasoning { .. }
        | StreamedAssistantContent::Final(_)
        | StreamedAssistantContent::Unknown(_) => Vec::new(),
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
    } = error.as_ref()
    else {
        return None;
    };
    Some((*max_turns, chat_history.as_ref().clone()))
}

pub fn checkpoint_messages(
    visible_history: &[rig::message::Message],
    full_history: &[rig::message::Message],
) -> Result<Vec<rig::message::Message>, AppError> {
    full_history
        .strip_prefix(visible_history)
        .filter(|messages| !messages.is_empty())
        .map(<[rig::message::Message]>::to_vec)
        .ok_or_else(|| AppError::Session("Budget checkpoint did not match the model-visible history".to_string()))
}

pub fn continuation_history(
    visible_history: &[rig::message::Message],
    checkpoint: &[rig::message::Message],
) -> Vec<rig::message::Message> {
    let mut history = Vec::with_capacity(visible_history.len() + checkpoint.len());
    history.extend_from_slice(visible_history);
    history.extend_from_slice(checkpoint);
    history
}

pub fn map_streaming_error(error: rig::agent::StreamingError) -> AppError {
    match error {
        rig::agent::StreamingError::Completion(error) => map_completion_error(error),
        rig::agent::StreamingError::Prompt(error) => map_prompt_error(*error),
    }
}

pub fn map_prompt_error(error: rig::completion::PromptError) -> AppError {
    match error {
        rig::completion::PromptError::MaxTurnsError { max_turns, .. } => AppError::ModelBudgetExhausted { max_turns },
        rig::completion::PromptError::PromptCancelled { reason, .. } => {
            AppError::Cancelled(super::helpers::redact_text(&reason))
        }
        rig::completion::PromptError::UnknownToolCall { tool_name, .. } => AppError::InvalidToolCall(tool_name),
        rig::completion::PromptError::CompletionError(error) => map_completion_error(error),
        rig::completion::PromptError::MemoryError(_) => AppError::Session("Conversation memory failed".to_string()),
    }
}

pub fn map_completion_error(error: rig::completion::CompletionError) -> AppError {
    if matches!(
        &error,
        rig::completion::CompletionError::ResponseError(message) if message.contains("ContentFilter")
    ) {
        return AppError::ContentFiltered;
    }
    let status = error.provider_response_status().map(|s| s.as_u16());
    match status {
        Some(code @ (401 | 403)) => AppError::Auth(format!("Model provider authentication failed (HTTP {code})")),
        Some(408 | 429 | 500..=599) => AppError::Network("Model provider request could not be completed".to_string()),
        Some(status) => AppError::Provider(format!("Model provider request failed with HTTP {status}")),
        None => AppError::Network("Model provider request failed without an HTTP status".to_string()),
    }
}
