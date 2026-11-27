pub use crate::provider::adapter::ModelHandle;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, ToolResult, ToolResultContent, UserContent};
use std::future::Future;
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
    let req = crate::provider::adapter::ModelCompletionRequest {
        preamble: Some(SUMMARIZATION_SYSTEM_PROMPT.to_string()),
        prompt: prompt.to_string(),
        messages: Vec::new(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: Some(0.0),
        thinking_level: None,
    };
    match tokio::time::timeout(Duration::from_secs(60), model.complete(req)).await {
        Ok(Ok(resp)) if !resp.content.trim().is_empty() => Ok((resp.content.trim().to_string(), resp.usage)),
        Ok(Ok(_)) => Err(LlmCallError::Other),
        Ok(Err(AppError::ContextOverflow(msg))) => Err(LlmCallError::ContextOverflow(msg)),
        Ok(Err(_)) => Err(LlmCallError::Other),
        Err(_) => Err(LlmCallError::Other),
    }
}

fn strip_json_fences(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(content) = rest.strip_suffix("```") {
            return content.trim();
        }
    } else if let Some(rest) = trimmed.strip_prefix("```")
        && let Some(content) = rest.strip_suffix("```")
    {
        return content.trim();
    }
    trimmed
}

async fn run_agent_extraction<T>(model: ModelHandle, prompt: &str) -> Result<T, LlmCallError>
where
    T: schemars::JsonSchema + for<'de> serde::Deserialize<'de> + serde::Serialize + Send + Sync + 'static,
{
    let schema = schemars::schema_for!(T);
    let schema_json = serde_json::to_string_pretty(&schema).unwrap_or_default();
    let full_prompt = format!(
        "{prompt}\n\nYou must return ONLY valid JSON matching this JSON schema:\n{schema_json}\nDo not include explanation, markdown formatting, or code fences, only the JSON object."
    );
    let (text, _) = run_agent_completion(model, &full_prompt).await?;
    let cleaned = strip_json_fences(&text);
    serde_json::from_str(cleaned).map_err(|_| LlmCallError::Other)
}

fn truncate_oldest_round(messages: &[ChatMessage]) -> Option<Vec<ChatMessage>> {
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

const MAX_OVERFLOW_RETRIES: usize = 3;

/// Calls `call` on `messages`, dropping the oldest round and retrying on context overflow.
/// Returns `None` when the call fails otherwise or no round is left to drop.
async fn retry_dropping_oldest_rounds<T, Fut>(
    mut messages: Vec<ChatMessage>,
    mut call: impl FnMut(&[ChatMessage]) -> Fut,
) -> Option<T>
where
    Fut: Future<Output = Result<T, LlmCallError>>,
{
    let mut retries = 0;
    loop {
        match call(&messages).await {
            Ok(value) => return Some(value),
            Err(LlmCallError::ContextOverflow(_)) if retries < MAX_OVERFLOW_RETRIES => {
                retries += 1;
                messages = truncate_oldest_round(&messages)?;
            }
            Err(_) => return None,
        }
    }
}

pub fn strip_media_for_summarization(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages.iter().map(strip_media_from_message).collect()
}

fn strip_media_from_user_content(item: &UserContent) -> UserContent {
    match item {
        UserContent::Image(img) => {
            let mime = img.media_type.as_deref().unwrap_or("unknown");
            UserContent::text(format!("[Image: {mime}]"))
        }
        UserContent::ToolResult(res) => {
            let new_res_content = res
                .content
                .iter()
                .map(|c| match c {
                    ToolResultContent::Image(img) => {
                        let mime = img.media_type.as_deref().unwrap_or("unknown");
                        ToolResultContent::text(format!("[Image: {mime}]"))
                    }
                    other => other.clone(),
                })
                .collect();
            UserContent::ToolResult(ToolResult {
                call: res.call.clone(),
                provider: res.provider.clone(),
                name: res.name.clone(),
                content: new_res_content,
            })
        }
        other => other.clone(),
    }
}

fn strip_media_from_message(msg: &ChatMessage) -> ChatMessage {
    match msg {
        ChatMessage::User { content } => {
            let new_content = content.iter().map(strip_media_from_user_content).collect();
            ChatMessage::User { content: new_content }
        }
        other => other.clone(),
    }
}

impl LlmCompactor {
    pub fn new(model: Option<ModelHandle>) -> Self {
        Self { model }
    }

    pub async fn extract<T>(&self, prompt: &str) -> Option<T>
    where
        T: schemars::JsonSchema + for<'de> serde::Deserialize<'de> + serde::Serialize + Send + Sync + 'static,
    {
        let model = self.model.as_ref()?.clone();
        run_agent_extraction(model, prompt).await.ok()
    }

    pub async fn summarize(&self, messages: &[ChatMessage], options: SummarizeOptions<'_>) -> String {
        self.summarize_with_usage(messages, options).await.0
    }

    pub async fn summarize_with_usage(
        &self,
        messages: &[ChatMessage],
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
        messages: &[ChatMessage],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        let attempt = match self.model.clone() {
            Some(model) => {
                let structured = options.structured;
                retry_dropping_oldest_rounds(strip_media_for_summarization(messages), |current| {
                    let prompt = Self::resolve_summary_prompt(&serialize_conversation(current), &options);
                    let model = model.clone();
                    async move { Self::execute_summary_call(model, &prompt, structured).await }
                })
                .await
            }
            None => None,
        };
        match attempt {
            Some(result) => result,
            None => (
                generate_fallback_summary(messages, options.prior_summary, options.custom_instructions),
                None,
            ),
        }
    }

    async fn summarize_prefix_with_usage(
        &self,
        prefix: &[ChatMessage],
        instructions: Option<&str>,
    ) -> (String, Option<StructuralUsage>) {
        let attempt = match self.model.clone() {
            Some(model) => {
                retry_dropping_oldest_rounds(strip_media_for_summarization(prefix), |current| {
                    let prompt = build_turn_prefix_prompt(&serialize_conversation(current), instructions);
                    let model = model.clone();
                    async move {
                        run_agent_completion(model, &prompt)
                            .await
                            .map(|(summary, usage)| (summary, Some(usage)))
                    }
                })
                .await
            }
            None => None,
        };
        match attempt {
            Some(result) => result,
            None => (generate_fallback_summary(prefix, None, instructions), None),
        }
    }

    async fn summarize_head_turn_with_usage(
        &self,
        messages: &[ChatMessage],
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
        messages: &[ChatMessage],
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
