use rig::message::Message;

use super::llm::{LlmCompactor, SummarizeOptions};
use crate::engine::AgentEngine;

impl AgentEngine {
    pub async fn summarize_branch(&self, messages: &[Message]) -> String {
        if messages.is_empty() {
            return String::new();
        }
        let compactor = LlmCompactor::new(self.model.clone());
        let summary = compactor
            .summarize(
                messages,
                SummarizeOptions {
                    prior_summary: None,
                    custom_instructions: Some(
                        "Summarize key discoveries, progress, decisions, and critical context from this abandoned branch before switching.",
                    ),
                    is_split_turn: false,
                },
            )
            .await;
        self.session_manager.redact_credentials(&summary)
    }
}
