use rig::agent::ModelHandle;
use rig::message::Message;
use std::time::Duration;

use rho_harness_core::session::compaction::{
    SUMMARIZATION_SYSTEM_PROMPT, build_summarization_prompt, build_turn_prefix_prompt,
    build_update_summarization_prompt, generate_fallback_summary, merge_split_turn_summary, serialize_conversation,
};
use rho_harness_core::tokens::is_user_turn_start;

pub struct LlmCompactor {
    model: Option<ModelHandle>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SummarizeOptions<'a> {
    pub prior_summary: Option<&'a str>,
    pub custom_instructions: Option<&'a str>,
    pub is_split_turn: bool,
}

impl LlmCompactor {
    pub fn new(model: Option<ModelHandle>) -> Self {
        Self { model }
    }

    pub async fn complete(&self, prompt: &str) -> Option<String> {
        let model = self.model.as_ref()?;
        let agent = rig::agent::AgentBuilder::from_model_handle(model.clone())
            .preamble(SUMMARIZATION_SYSTEM_PROMPT)
            .default_max_turns(1)
            .record_content_telemetry(false)
            .build();
        let runner = crate::engine::runtime::build_runner(&agent, prompt).max_turns(1);
        let completion_future = runner.run();
        match tokio::time::timeout(Duration::from_secs(60), completion_future).await {
            Ok(Ok(response)) => {
                let trimmed = response.output.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Ok(Err(err)) => {
                eprintln!("Warning: Compaction LLM call failed, falling back to deterministic summary: {err}");
                None
            }
            Err(_) => {
                eprintln!("Warning: Compaction LLM call timed out after 60s, falling back to deterministic summary");
                None
            }
        }
    }

    pub async fn summarize(&self, messages: &[Message], options: SummarizeOptions<'_>) -> String {
        if messages.is_empty() {
            return options.prior_summary.unwrap_or_default().to_string();
        }

        if options.is_split_turn {
            self.summarize_split_turn(messages, options).await
        } else {
            self.summarize_full(messages, options).await
        }
    }

    async fn summarize_full(&self, messages: &[Message], options: SummarizeOptions<'_>) -> String {
        let transcript = serialize_conversation(messages);
        let prompt = match options.prior_summary {
            Some(prior) => build_update_summarization_prompt(&transcript, prior, options.custom_instructions),
            None => build_summarization_prompt(&transcript, options.custom_instructions),
        };

        if let Some(summary) = self.complete(&prompt).await {
            summary
        } else {
            generate_fallback_summary(messages, options.prior_summary, options.custom_instructions)
        }
    }

    async fn summarize_split_turn(&self, messages: &[Message], options: SummarizeOptions<'_>) -> String {
        let split_turn_start = messages.iter().rposition(is_user_turn_start).unwrap_or(0);

        if split_turn_start > 0 {
            let earlier = &messages[..split_turn_start];
            let prefix = &messages[split_turn_start..];

            let main_summary = self.summarize_full(earlier, options).await;

            let prefix_transcript = serialize_conversation(prefix);
            let prefix_prompt = build_turn_prefix_prompt(&prefix_transcript, options.custom_instructions);
            let prefix_summary = if let Some(summary) = self.complete(&prefix_prompt).await {
                summary
            } else {
                generate_fallback_summary(prefix, None, options.custom_instructions)
            };

            merge_split_turn_summary(&main_summary, &prefix_summary)
        } else {
            let prefix_transcript = serialize_conversation(messages);
            let prefix_prompt = build_turn_prefix_prompt(&prefix_transcript, options.custom_instructions);
            let prefix_summary = if let Some(summary) = self.complete(&prefix_prompt).await {
                summary
            } else {
                generate_fallback_summary(messages, None, options.custom_instructions)
            };

            match options.prior_summary {
                Some(prior) => merge_split_turn_summary(prior, &prefix_summary),
                None => prefix_summary,
            }
        }
    }
}
