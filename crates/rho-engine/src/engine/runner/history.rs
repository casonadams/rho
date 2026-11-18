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

pub fn continuation_history(visible_history: &[ChatMessage], checkpoint: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut history = Vec::with_capacity(visible_history.len() + checkpoint.len());
    history.extend_from_slice(visible_history);
    history.extend_from_slice(checkpoint);
    history
}

fn has_tool_calls(message: &ChatMessage) -> bool {
    matches!(message, ChatMessage::Assistant { content, .. }
        if content.iter().any(|c| matches!(c, rho_harness_core::model::AssistantContent::ToolCall(_))))
}

/// Drops trailing assistant messages whose tool calls never got results, which providers reject.
pub fn without_unanswered_tool_calls(history: &[ChatMessage]) -> &[ChatMessage] {
    let end = history.iter().rposition(|m| !has_tool_calls(m)).map_or(0, |i| i + 1);
    &history[..end]
}
