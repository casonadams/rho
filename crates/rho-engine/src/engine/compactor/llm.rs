use rig::agent::ModelHandle;
use rig::completion::message::MimeType;
use rig::message::{Message, ToolResultContent, UserContent};
use std::time::Duration;

use rho_harness_core::session::compaction::{
    CompactionSummaryPayload, SUMMARIZATION_SYSTEM_PROMPT, build_summarization_prompt, build_turn_prefix_prompt,
    build_update_summarization_prompt, generate_fallback_summary, merge_split_turn_summary, serialize_conversation,
};
use rho_harness_core::tokens::is_user_turn_start;

use crate::engine::metrics::StructuralUsage;

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

async fn run_agent_completion(model: ModelHandle, prompt: &str) -> Result<(String, StructuralUsage), LlmCallError> {
    let agent = rig::agent::AgentBuilder::from_model_handle(model)
        .preamble(SUMMARIZATION_SYSTEM_PROMPT)
        .default_max_turns(1)
        .record_content_telemetry(false)
        .build();
    let runner = crate::engine::runtime::build_runner(&agent, prompt).max_turns(1);
    match tokio::time::timeout(Duration::from_secs(60), runner.run()).await {
        Ok(Ok(resp)) if !resp.output.trim().is_empty() => Ok((resp.output.trim().to_string(), resp.usage.into())),
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

pub fn strip_media_for_summarization(messages: &[Message]) -> Vec<Message> {
    messages.iter().map(strip_media_from_message).collect()
}

fn strip_media_from_user_content(item: &UserContent) -> UserContent {
    match item {
        UserContent::Image(img) => {
            let mime = img.media_type.as_ref().map_or("unknown", MimeType::to_mime_type);
            UserContent::text(format!("[Image: {mime}]"))
        }
        UserContent::ToolResult(res) => {
            let new_res_content = res
                .content
                .iter()
                .map(|c| match c {
                    ToolResultContent::Image(img) => {
                        let mime = img.media_type.as_ref().map_or("unknown", MimeType::to_mime_type);
                        ToolResultContent::text(format!("[Image: {mime}]"))
                    }
                    other => other.clone(),
                })
                .collect();
            UserContent::ToolResult(rig::message::ToolResult {
                call: res.call.clone(),
                provider: res.provider.clone(),
                name: res.name.clone(),
                content: new_res_content,
            })
        }
        other => other.clone(),
    }
}

fn strip_media_from_message(msg: &Message) -> Message {
    match msg {
        Message::User { content } => {
            let new_content = content.iter().map(strip_media_from_user_content).collect();
            Message::User { content: new_content }
        }
        other => other.clone(),
    }
}

impl LlmCompactor {
    pub fn new(model: Option<ModelHandle>) -> Self {
        Self { model }
    }

    pub async fn complete(&self, prompt: &str) -> Option<String> {
        let model = self.model.as_ref()?.clone();
        run_agent_completion(model, prompt).await.ok().map(|(text, _)| text)
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
        self.summarize_with_usage(messages, options).await.0
    }

    pub async fn summarize_with_usage(
        &self,
        messages: &[Message],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        if messages.is_empty() {
            return (options.prior_summary.unwrap_or_default().to_string(), None);
        }

        if options.is_split_turn {
            self.summarize_split_turn_with_usage(messages, options).await
        } else {
            self.summarize_full_with_usage(messages, options).await
        }
    }

    fn resolve_summary_prompt(transcript: &str, options: &SummarizeOptions<'_>) -> String {
        match options.prior_summary {
            Some(prior) => build_update_summarization_prompt(transcript, prior, options.custom_instructions),
            None => build_summarization_prompt(transcript, options.custom_instructions),
        }
    }

    async fn execute_summary_call(
        model: ModelHandle,
        prompt: &str,
        structured: bool,
    ) -> Result<(String, Option<StructuralUsage>), LlmCallError> {
        if structured {
            match run_agent_extraction::<CompactionSummaryPayload>(model.clone(), prompt).await {
                Ok(payload) => Ok((payload.render_markdown(), None)),
                Err(LlmCallError::ContextOverflow(msg)) => Err(LlmCallError::ContextOverflow(msg)),
                Err(LlmCallError::Other) => {
                    let (summary, usage) = run_agent_completion(model, prompt).await?;
                    Ok((summary, Some(usage)))
                }
            }
        } else {
            let (summary, usage) = run_agent_completion(model, prompt).await?;
            Ok((summary, Some(usage)))
        }
    }

    async fn summarize_full_with_usage(
        &self,
        messages: &[Message],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        const MAX_OVERFLOW_RETRIES: usize = 3;
        let mut current_messages = strip_media_for_summarization(messages);

        for attempt in 0..=MAX_OVERFLOW_RETRIES {
            let transcript = serialize_conversation(&current_messages);
            let prompt = Self::resolve_summary_prompt(&transcript, &options);

            let Some(model) = self.model.as_ref().cloned() else {
                break;
            };

            match Self::execute_summary_call(model, &prompt, options.structured).await {
                Ok((summary, usage)) => return (summary, usage),
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

        (
            generate_fallback_summary(messages, options.prior_summary, options.custom_instructions),
            None,
        )
    }

    async fn summarize_prefix_with_usage(
        &self,
        prefix: &[Message],
        instructions: Option<&str>,
    ) -> (String, Option<StructuralUsage>) {
        const MAX_OVERFLOW_RETRIES: usize = 3;
        let mut current_messages = strip_media_for_summarization(prefix);

        for attempt in 0..=MAX_OVERFLOW_RETRIES {
            let transcript = serialize_conversation(&current_messages);
            let prompt = build_turn_prefix_prompt(&transcript, instructions);
            let Some(model) = self.model.as_ref().cloned() else {
                break;
            };

            match run_agent_completion(model, &prompt).await {
                Ok((summary, usage)) => return (summary, Some(usage)),
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

        (generate_fallback_summary(prefix, None, instructions), None)
    }

    async fn summarize_head_turn_with_usage(
        &self,
        messages: &[Message],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        let (prefix_summary, usage) = self
            .summarize_prefix_with_usage(messages, options.custom_instructions)
            .await;
        let summary = match options.prior_summary {
            Some(prior) => merge_split_turn_summary(prior, &prefix_summary),
            None => prefix_summary,
        };
        (summary, usage)
    }

    async fn summarize_split_turn_with_usage(
        &self,
        messages: &[Message],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        let split = messages.iter().rposition(is_user_turn_start).unwrap_or(0);
        if split > 0 {
            let (main_summary, usage1) = self.summarize_full_with_usage(&messages[..split], options).await;
            let (prefix_summary, usage2) = self
                .summarize_prefix_with_usage(&messages[split..], options.custom_instructions)
                .await;
            let merged_usage = match (usage1, usage2) {
                (Some(u1), Some(u2)) => Some(u1.merge(&u2)),
                (Some(u1), None) => Some(u1),
                (None, Some(u2)) => Some(u2),
                (None, None) => None,
            };
            (merge_split_turn_summary(&main_summary, &prefix_summary), merged_usage)
        } else {
            self.summarize_head_turn_with_usage(messages, options).await
        }
    }
}
