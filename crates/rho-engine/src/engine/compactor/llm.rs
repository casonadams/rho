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

async fn run_agent_completion(model: ModelHandle, prompt: &str) -> Option<String> {
    let agent = rig::agent::AgentBuilder::from_model_handle(model)
        .preamble(SUMMARIZATION_SYSTEM_PROMPT)
        .default_max_turns(1)
        .record_content_telemetry(false)
        .build();
    let runner = crate::engine::runtime::build_runner(&agent, prompt).max_turns(1);
    match tokio::time::timeout(Duration::from_secs(60), runner.run()).await {
        Ok(Ok(resp)) if !resp.output.trim().is_empty() => Some(resp.output.trim().to_string()),
        Ok(Ok(_)) => None,
        Ok(Err(e)) => {
            eprintln!("Warning: Compaction LLM call failed, falling back to deterministic summary: {e}");
            None
        }
        Err(_) => {
            eprintln!("Warning: Compaction LLM call timed out after 60s, falling back to deterministic summary");
            None
        }
    }
}

impl LlmCompactor {
    pub fn new(model: Option<ModelHandle>) -> Self {
        Self { model }
    }

    pub async fn complete(&self, prompt: &str) -> Option<String> {
        let model = self.model.as_ref()?.clone();
        run_agent_completion(model, prompt).await
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

    async fn summarize_prefix(&self, prefix: &[Message], instructions: Option<&str>) -> String {
        let transcript = serialize_conversation(prefix);
        let prompt = build_turn_prefix_prompt(&transcript, instructions);
        match self.complete(&prompt).await {
            Some(summary) => summary,
            None => generate_fallback_summary(prefix, None, instructions),
        }
    }

    async fn summarize_head_turn(&self, messages: &[Message], options: SummarizeOptions<'_>) -> String {
        let prefix_summary = self.summarize_prefix(messages, options.custom_instructions).await;
        match options.prior_summary {
            Some(prior) => merge_split_turn_summary(prior, &prefix_summary),
            None => prefix_summary,
        }
    }

    async fn summarize_split_turn(&self, messages: &[Message], options: SummarizeOptions<'_>) -> String {
        let split = messages.iter().rposition(is_user_turn_start).unwrap_or(0);
        if split > 0 {
            let main_summary = self.summarize_full(&messages[..split], options).await;
            let prefix_summary = self
                .summarize_prefix(&messages[split..], options.custom_instructions)
                .await;
            merge_split_turn_summary(&main_summary, &prefix_summary)
        } else {
            self.summarize_head_turn(messages, options).await
        }
    }
}
