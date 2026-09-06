use crate::error::Result;
use rig::message::{AssistantContent, Message, UserContent};
use std::collections::HashSet;

use super::session_error;

/// Tracks tool-call identities already committed to the canonical history so
/// appended batches can be validated without rescanning every message.
///
/// Appended batches validate standalone: the prefix always ends with all tool
/// calls paired, so batch-local checks plus the persistent id set reproduce
/// full-history semantics exactly.
#[derive(Debug, Default, Clone)]
pub(crate) struct CanonicalHistory {
    seen_calls: HashSet<String>,
}

fn validate_assistant_content(
    content: &[AssistantContent],
    (seen_calls, batch_calls): (&HashSet<String>, &mut HashSet<String>),
    pending: &mut Vec<(String, String)>,
) -> Result<()> {
    if content.is_empty() || !pending.is_empty() {
        return Err(session_error("canonical message role ordering is invalid"));
    }
    for item in content {
        if let AssistantContent::ToolCall(call) = item {
            let call_id = call.id.to_string();
            if seen_calls.contains(&call_id) || !batch_calls.insert(call_id.clone()) {
                return Err(session_error("canonical tool-call id is duplicated"));
            }
            pending.push((call_id, call.function.name.clone()));
        }
    }
    Ok(())
}

fn check_trailing_state(
    pending: &[(String, String)],
    (require_assistant_end, is_empty, last_was_assistant): (bool, bool, bool),
) -> Result<()> {
    if !pending.is_empty() {
        return Err(session_error("canonical history contains a dangling tool call"));
    }
    if require_assistant_end && !is_empty && !last_was_assistant {
        return Err(session_error(
            "canonical history does not end with an assistant message",
        ));
    }
    Ok(())
}

impl CanonicalHistory {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn clear(&mut self) {
        self.seen_calls.clear();
    }

    /// Validate a batch that will be joined to the canonical history and adopt
    /// its tool-call ids on success.
    pub(crate) fn check_canonical_batch(&mut self, messages: &[Message]) -> Result<()> {
        let batch_calls = self.check_history(messages, true)?;
        self.seen_calls.extend(batch_calls);
        Ok(())
    }

    /// Validate a candidate checkpoint without adopting its tool-call ids;
    /// checkpoints only merge into the canonical history at promotion.
    pub(crate) fn check_checkpoint_batch(&self, messages: &[Message]) -> Result<()> {
        self.check_history(messages, false).map(|_| ())
    }

    fn scan_history_message(
        &self,
        message: &Message,
        (pending, batch_calls, last_was_assistant): (&mut Vec<(String, String)>, &mut HashSet<String>, &mut bool),
    ) -> Result<()> {
        match message {
            Message::System { .. } => Err(session_error("system messages are not canonical conversation memory")),
            Message::User { content } => {
                if content.is_empty() {
                    return Err(session_error("canonical message role ordering is invalid"));
                }
                validate_user_content(content, pending)?;
                *last_was_assistant = false;
                Ok(())
            }
            Message::Assistant { content, .. } => {
                validate_assistant_content(content, (&self.seen_calls, batch_calls), pending)?;
                *last_was_assistant = true;
                Ok(())
            }
        }
    }

    fn scan_history_messages(
        &self,
        messages: &[Message],
        mut state: (&mut Vec<(String, String)>, &mut HashSet<String>, &mut bool),
    ) -> Result<()> {
        for message in messages {
            self.scan_history_message(message, (&mut state.0, &mut state.1, &mut state.2))?;
        }
        Ok(())
    }

    fn check_history(&self, messages: &[Message], require_assistant_end: bool) -> Result<HashSet<String>> {
        let mut pending: Vec<(String, String)> = Vec::new();
        let mut batch_calls = HashSet::new();
        let mut last_was_assistant = false;

        self.scan_history_messages(messages, (&mut pending, &mut batch_calls, &mut last_was_assistant))?;
        check_trailing_state(
            &pending,
            (require_assistant_end, messages.is_empty(), last_was_assistant),
        )?;
        Ok(batch_calls)
    }
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
