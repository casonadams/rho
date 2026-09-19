use rig::agent::ModelHandle;
use rig::message::Message;
use std::time::Duration;

use rho_harness_core::session::compaction::{
    CompactionSummaryPayload, SUMMARIZATION_SYSTEM_PROMPT, build_summarization_prompt, build_turn_prefix_prompt,
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
    pub structured: bool,
}

#[derive(Debug)]
enum LlmCallError {
    ContextOverflow(String),
    Other,
}

async fn run_agent_completion(model: ModelHandle, prompt: &str) -> Result<String, LlmCallError> {
    let agent = rig::agent::AgentBuilder::from_model_handle(model)
        .preamble(SUMMARIZATION_SYSTEM_PROMPT)
        .default_max_turns(1)
        .record_content_telemetry(false)
        .build();
    let runner = crate::engine::runtime::build_runner(&agent, prompt).max_turns(1);
    match tokio::time::timeout(Duration::from_secs(60), runner.run()).await {
        Ok(Ok(resp)) if !resp.output.trim().is_empty() => Ok(resp.output.trim().to_string()),
        Ok(Ok(_)) => Err(LlmCallError::Other),
        Ok(Err(e)) => {
            let msg = e.to_string();
            if crate::engine::compactor::is_context_overflow_message(&msg) {
                Err(LlmCallError::ContextOverflow(msg))
            } else {
                eprintln!("Warning: Compaction LLM call failed, falling back to deterministic summary: {e}");
                Err(LlmCallError::Other)
            }
        }
        Err(_) => {
            eprintln!("Warning: Compaction LLM call timed out after 60s, falling back to deterministic summary");
            Err(LlmCallError::Other)
        }
    }
}

async fn run_agent_extraction<T>(model: ModelHandle, prompt: &str) -> Result<T, LlmCallError>
where
    T: schemars::JsonSchema
        + for<'de> serde::Deserialize<'de>
        + serde::Serialize
        + rig::wasm_compat::WasmCompatSend
        + rig::wasm_compat::WasmCompatSync
        + 'static,
{
    let extractor = rig::extractor::ExtractorBuilder::<T>::new(model).build();
    match tokio::time::timeout(Duration::from_secs(60), extractor.extract(prompt)).await {
        Ok(Ok(val)) => Ok(val),
        Ok(Err(e)) => {
            let msg = e.to_string();
            if crate::engine::compactor::is_context_overflow_message(&msg) {
                Err(LlmCallError::ContextOverflow(msg))
            } else {
                eprintln!("Warning: Extraction failed: {e}");
                Err(LlmCallError::Other)
            }
        }
        Err(_) => {
            eprintln!("Warning: Extraction timed out after 60s");
            Err(LlmCallError::Other)
        }
    }
}

fn truncate_oldest_round(messages: &[Message]) -> Option<Vec<Message>> {
    let first_user_idx = messages.iter().position(is_user_turn_start)?;
    let second_user_idx = messages[first_user_idx + 1..]
        .iter()
        .position(is_user_turn_start)
        .map(|i| first_user_idx + 1 + i)?;

    let mut truncated = Vec::with_capacity(messages.len() - (second_user_idx - first_user_idx));
    if first_user_idx > 0 {
        truncated.extend_from_slice(&messages[..first_user_idx]);
    }
    truncated.extend_from_slice(&messages[second_user_idx..]);
    Some(truncated)
}

impl LlmCompactor {
    pub fn new(model: Option<ModelHandle>) -> Self {
        Self { model }
    }

    pub async fn complete(&self, prompt: &str) -> Option<String> {
        let model = self.model.as_ref()?.clone();
        run_agent_completion(model, prompt).await.ok()
    }

    pub async fn extract<T>(&self, prompt: &str) -> Option<T>
    where
        T: schemars::JsonSchema
            + for<'de> serde::Deserialize<'de>
            + serde::Serialize
            + rig::wasm_compat::WasmCompatSend
            + rig::wasm_compat::WasmCompatSync
            + 'static,
    {
        let model = self.model.as_ref()?.clone();
        run_agent_extraction(model, prompt).await.ok()
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
        const MAX_OVERFLOW_RETRIES: usize = 3;
        let mut current_messages = messages.to_vec();

        for attempt in 0..=MAX_OVERFLOW_RETRIES {
            let transcript = serialize_conversation(&current_messages);
            let prompt = match options.prior_summary {
                Some(prior) => build_update_summarization_prompt(&transcript, prior, options.custom_instructions),
                None => build_summarization_prompt(&transcript, options.custom_instructions),
            };

            let Some(model) = self.model.as_ref().cloned() else {
                break;
            };

            let result = if options.structured {
                match run_agent_extraction::<CompactionSummaryPayload>(model.clone(), &prompt).await {
                    Ok(payload) => return payload.render_markdown(),
                    Err(LlmCallError::ContextOverflow(msg)) => Err(LlmCallError::ContextOverflow(msg)),
                    Err(LlmCallError::Other) => run_agent_completion(model, &prompt).await,
                }
            } else {
                run_agent_completion(model, &prompt).await
            };

            match result {
                Ok(summary) => return summary,
                Err(LlmCallError::ContextOverflow(_)) if attempt < MAX_OVERFLOW_RETRIES => {
                    if let Some(truncated) = truncate_oldest_round(&current_messages) {
                        current_messages = truncated;
                        continue;
                    }
                    break;
                }
                Err(_) => break,
            }
        }

        generate_fallback_summary(messages, options.prior_summary, options.custom_instructions)
    }

    async fn summarize_prefix(&self, prefix: &[Message], instructions: Option<&str>) -> String {
        const MAX_OVERFLOW_RETRIES: usize = 3;
        let mut current_messages = prefix.to_vec();

        for attempt in 0..=MAX_OVERFLOW_RETRIES {
            let transcript = serialize_conversation(&current_messages);
            let prompt = build_turn_prefix_prompt(&transcript, instructions);
            let Some(model) = self.model.as_ref().cloned() else {
                break;
            };

            match run_agent_completion(model, &prompt).await {
                Ok(summary) => return summary,
                Err(LlmCallError::ContextOverflow(_)) if attempt < MAX_OVERFLOW_RETRIES => {
                    if let Some(truncated) = truncate_oldest_round(&current_messages) {
                        current_messages = truncated;
                        continue;
                    }
                    break;
                }
                Err(_) => break,
            }
        }

        generate_fallback_summary(prefix, None, instructions)
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
