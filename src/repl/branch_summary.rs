//! Branch-switch rules shared by the line and live frontends.

use crate::engine::AgentEngine;
use rho_harness_core::model::ChatMessage;
use rho_harness_core::session::TreeNodeKind;
use rho_harness_core::session::tree::TreeNodeData;

pub(crate) const SUMMARY_PROMPT: &str = "Summarize discoveries from abandoned branch before switching? [Y/n]: ";

pub(crate) fn has_assistant_turn(abandoned: &[&TreeNodeData]) -> bool {
    abandoned.iter().any(|n| n.kind == TreeNodeKind::AssistantTurn)
}

pub(crate) fn should_summarize_branch(input: &str) -> bool {
    let trimmed = input.trim().to_lowercase();
    trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
}

pub(crate) fn abandoned_messages(abandoned: &[&TreeNodeData]) -> Vec<ChatMessage> {
    abandoned.iter().flat_map(|n| n.messages.clone()).collect()
}

pub(crate) async fn summarize_abandoned(
    engine: &AgentEngine,
    abandoned: &[&TreeNodeData],
    accepted: bool,
) -> Option<String> {
    if !accepted {
        return None;
    }
    Some(engine.summarize_branch(&abandoned_messages(abandoned)).await)
}
