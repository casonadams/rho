use crate::error::Result;
use rig::message::{AssistantContent, Message, UserContent};
use std::collections::HashSet;

use super::session_error;

pub(crate) fn validate_canonical_history(messages: &[Message]) -> Result<()> {
    validate_history(messages, true)
}

pub(crate) fn validate_checkpoint_history(messages: &[Message]) -> Result<()> {
    validate_history(messages, false)
}

fn validate_history(messages: &[Message], require_assistant_end: bool) -> Result<()> {
    let mut seen_calls = HashSet::new();
    let mut pending = Vec::new();
    let mut last_was_assistant = false;
    for message in messages {
        match message {
            Message::System { .. } => {
                return Err(session_error("system messages are not canonical conversation memory"));
            }
            Message::User { content } => {
                if content.is_empty() {
                    return Err(session_error("canonical message role ordering is invalid"));
                }
                validate_user_content(content, &mut pending)?;
                last_was_assistant = false;
            }
            Message::Assistant { content, .. } => {
                if content.is_empty() || !pending.is_empty() {
                    return Err(session_error("canonical message role ordering is invalid"));
                }
                for item in content {
                    if let AssistantContent::ToolCall(call) = item {
                        if !seen_calls.insert(call.id.to_string()) {
                            return Err(session_error("canonical tool-call id is duplicated"));
                        }
                        pending.push((call.id.to_string(), call.function.name.clone()));
                    }
                }
                last_was_assistant = true;
            }
        }
    }
    if !pending.is_empty() {
        return Err(session_error("canonical history contains a dangling tool call"));
    }
    if require_assistant_end && !messages.is_empty() && !last_was_assistant {
        return Err(session_error(
            "canonical history does not end with an assistant message",
        ));
    }
    Ok(())
}

fn validate_user_content(content: &[UserContent], pending: &mut Vec<(String, String)>) -> Result<()> {
    let results = content
        .iter()
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some((result.call.as_str(), result.name.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        if !results.is_empty() {
            return Err(session_error("canonical history contains an orphaned tool result"));
        }
        return Ok(());
    }
    if results.len() != content.len() || results.len() != pending.len() {
        return Err(session_error(
            "canonical tool calls do not have exactly one result each",
        ));
    }
    for ((result_id, result_name), (call_id, call_name)) in results.iter().zip(pending.iter()) {
        if *result_id != call_id || *result_name != call_name {
            return Err(session_error("canonical tool-call/result correlation is invalid"));
        }
    }
    pending.clear();
    Ok(())
}
