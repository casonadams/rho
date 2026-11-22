//! What a subagent round takes and returns: its input, the task prompt and parent conversation it
//! starts from, and the result the delegating agent receives.

use super::evidence::{ToolOutcome, problem_count};
use super::observed::ASK_PARENT_TOOL;
use super::workspace::WorkspaceChange;
use rho_harness_core::model::{AssistantContent, ChatMessage, UserContent};
use std::collections::BTreeSet;

const MAX_PARENT_MESSAGES: usize = 20;
const MAX_PARENT_MESSAGE_CHARS: usize = 1_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundEnd {
    Answered(String),
    /// The subagent called `ask_parent` and is waiting for a reply.
    Asked(String),
    /// Stopped before a final answer (turn limit or empty reply) and summarized its work so far.
    Stopped {
        reason: String,
        summary: String,
    },
    /// A model call failed after the subagent gathered findings, which the text reports.
    Partial(String),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Round {
    pub end: RoundEnd,
    pub transcript: Vec<ChatMessage>,
    pub turns: usize,
    pub tool_calls: usize,
    pub tokens: u64,
    pub cached_tokens: u64,
    pub changed_files: BTreeSet<String>,
    /// Every uncommitted-file change in the git work tree during the round, whoever made it.
    pub workspace_changes: BTreeSet<WorkspaceChange>,
    /// False when the workspace comparison could not cover every file (a non-git walk hit its limit).
    pub workspace_complete: bool,
    pub tool_log: Vec<ToolOutcome>,
    /// Files the subagent read, wrote, or edited, relative to the workspace root.
    pub touched_files: BTreeSet<String>,
    /// The transcript was compacted, so it replaces the saved conversation instead of extending it.
    pub compacted: bool,
}

impl Round {
    #[must_use]
    pub fn problem_count(&self) -> usize {
        problem_count(&self.tool_log)
    }

    /// Ended early with work the parent can build on or continue.
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        matches!(
            self.end,
            RoundEnd::Asked(_) | RoundEnd::Stopped { .. } | RoundEnd::Partial(_)
        )
    }

    /// The text the parent receives above the footer, or the error when the round produced nothing usable.
    pub fn body(&self) -> Result<String, String> {
        match &self.end {
            RoundEnd::Answered(text) => Ok(text.trim().to_string()),
            RoundEnd::Asked(question) => Ok(format!("Question for you: {question}")),
            RoundEnd::Stopped { reason, summary } => Ok(format!(
                "Incomplete: subagent {reason} before finishing; this summary covers its work so far.\n\n{summary}"
            )),
            RoundEnd::Partial(text) => Ok(text.clone()),
            RoundEnd::Failed(error) => Err(error.clone()),
            RoundEnd::Cancelled => Err("Subagent interrupted by operator".to_string()),
        }
    }
}

/// What a round starts from: a new instruction, or a reply to the subagent's `ask_parent` question.
#[derive(Debug, Clone, Copy)]
pub enum RoundInput<'a> {
    Prompt(&'a str),
    Reply(&'a str),
}

/// Whether the conversation's last assistant message asked the delegating agent a question.
pub(super) fn ended_with_question(transcript: &[ChatMessage]) -> bool {
    let last_assistant = transcript
        .iter()
        .rev()
        .find(|m| matches!(m, ChatMessage::Assistant { .. }));
    let Some(ChatMessage::Assistant { content, .. }) = last_assistant else {
        return false;
    };
    content
        .iter()
        .any(|c| matches!(c, AssistantContent::ToolCall(call) if call.function.name == ASK_PARENT_TOOL))
}

pub fn build_subagent_prompt(prompt: &str, context_slice: Option<&str>) -> String {
    match context_slice {
        Some(slice) if !slice.trim().is_empty() => {
            format!("Context:\n{}\n\nTask:\n{}", slice.trim(), prompt.trim())
        }
        _ => prompt.trim().to_string(),
    }
}

fn message_text(message: &ChatMessage) -> Option<(&'static str, String)> {
    let (speaker, text) = match message {
        ChatMessage::User { content } => (
            "User",
            content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::Assistant { content, .. } => (
            "Assistant",
            content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ChatMessage::System { .. } => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| (speaker, truncate_middle(text)))
}

fn truncate_middle(text: &str) -> String {
    let total = text.chars().count();
    if total <= MAX_PARENT_MESSAGE_CHARS {
        return text.to_string();
    }
    let head_len = MAX_PARENT_MESSAGE_CHARS / 2;
    let tail_len = MAX_PARENT_MESSAGE_CHARS - head_len;
    let head: String = text.chars().take(head_len).collect();
    let tail: String = text.chars().skip(total - tail_len).collect();
    format!(
        "{head}\n[... {} characters omitted ...]\n{tail}",
        total - MAX_PARENT_MESSAGE_CHARS
    )
}

/// Text-only transcript of the most recent parent messages; tool calls, tool output and reasoning are left out.
pub(crate) fn render_parent_conversation(history: &[ChatMessage]) -> String {
    let lines: Vec<String> = history
        .iter()
        .filter_map(message_text)
        .map(|(speaker, text)| format!("{speaker}: {text}"))
        .collect();
    let recent = &lines[lines.len().saturating_sub(MAX_PARENT_MESSAGES)..];
    if recent.is_empty() {
        return String::new();
    }
    format!("Parent conversation (most recent last):\n{}", recent.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_formatting() {
        assert_eq!(build_subagent_prompt("do work", None), "do work");
        assert_eq!(build_subagent_prompt("do work", Some("")), "do work");
        assert_eq!(
            build_subagent_prompt("do work", Some("important background")),
            "Context:\nimportant background\n\nTask:\ndo work"
        );
    }

    #[test]
    fn a_reply_is_framed_as_one_only_after_a_question() {
        let asked = ChatMessage::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(rho_harness_core::model::ToolCall::new(
                "q1",
                rho_harness_core::model::ToolFunction::new(ASK_PARENT_TOOL, serde_json::json!({})),
            ))],
        };
        assert!(ended_with_question(&[ChatMessage::user("task"), asked.clone()]));
        assert!(!ended_with_question(&[asked, ChatMessage::assistant("answer")]));
        assert!(!ended_with_question(&[]));
    }
}
