pub use crate::provider::adapter::ModelHandle;
use rho_harness_core::error::AppError;
use rho_harness_core::model::{ChatMessage, ToolResult, ToolResultContent, UserContent};
use std::future::Future;
use std::time::Duration;

use rho_harness_core::session::compaction::{
    SUMMARIZATION_SYSTEM_PROMPT, build_summarization_prompt, build_turn_prefix_prompt,
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
}

#[derive(Debug)]
enum LlmCallError {
    ContextOverflow(AppError),
    Failed(AppError),
}

impl std::fmt::Display for LlmCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextOverflow(cause) | Self::Failed(cause) => cause.fmt(f),
        }
    }
}

const SUMMARY_TIMEOUT: Duration = Duration::from_secs(60);

async fn run_agent_completion(model: ModelHandle, prompt: &str) -> Result<(String, StructuralUsage), LlmCallError> {
    let req = crate::provider::adapter::ModelCompletionRequest {
        preamble: Some(SUMMARIZATION_SYSTEM_PROMPT.to_string()),
        prompt: prompt.to_string(),
        messages: Default::default(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: Some(0.0),
        thinking_level: None,
        conversation_id: None,
    };
    let response = tokio::time::timeout(SUMMARY_TIMEOUT, model.complete(req))
        .await
        .map_err(|_| {
            AppError::Provider(format!(
                "summary request timed out after {}s",
                SUMMARY_TIMEOUT.as_secs()
            ))
        });
    match response {
        Ok(Ok(resp)) if !resp.content.trim().is_empty() => Ok((resp.content.trim().to_string(), resp.usage)),
        Ok(Ok(_)) => Err(LlmCallError::Failed(AppError::Provider(
            "summary model returned no text".to_string(),
        ))),
        Ok(Err(error @ AppError::ContextOverflow(_))) => Err(LlmCallError::ContextOverflow(error)),
        Ok(Err(error)) | Err(error) => Err(LlmCallError::Failed(error)),
    }
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
/// Returns the last failure when the call fails otherwise or no round is left to drop.
async fn retry_dropping_oldest_rounds<T, Fut>(
    mut messages: Vec<ChatMessage>,
    mut call: impl FnMut(&[ChatMessage]) -> Fut,
) -> Result<T, LlmCallError>
where
    Fut: Future<Output = Result<T, LlmCallError>>,
{
    let mut retries = 0;
    loop {
        match call(&messages).await {
            Ok(value) => return Ok(value),
            Err(overflow @ LlmCallError::ContextOverflow(_)) if retries < MAX_OVERFLOW_RETRIES => {
                let Some(shorter) = truncate_oldest_round(&messages) else {
                    return Err(overflow);
                };
                retries += 1;
                messages = shorter;
            }
            Err(error) => return Err(error),
        }
    }
}

fn summary_or_fallback(
    attempt: Result<(String, Option<StructuralUsage>), LlmCallError>,
    fallback: impl FnOnce() -> String,
) -> (String, Option<StructuralUsage>) {
    match attempt {
        Ok(result) => result,
        Err(error) => {
            tracing::warn!("LLM compaction failed, using a deterministic summary: {error}");
            (fallback(), None)
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

    async fn summarize_full_with_usage(
        &self,
        messages: &[ChatMessage],
        options: SummarizeOptions<'_>,
    ) -> (String, Option<StructuralUsage>) {
        self.summarize_with(
            messages,
            |transcript| Self::resolve_summary_prompt(transcript, &options),
            || generate_fallback_summary(messages, options.prior_summary, options.custom_instructions),
        )
        .await
    }

    async fn summarize_prefix_with_usage(
        &self,
        prefix: &[ChatMessage],
        instructions: Option<&str>,
    ) -> (String, Option<StructuralUsage>) {
        self.summarize_with(
            prefix,
            |transcript| build_turn_prefix_prompt(transcript, instructions),
            || generate_fallback_summary(prefix, None, instructions),
        )
        .await
    }

    /// Summarizes `messages` with the prompt `prompt` builds from their transcript, dropping the
    /// oldest rounds while the request is too long; `fallback` when there is no model or it fails.
    async fn summarize_with(
        &self,
        messages: &[ChatMessage],
        prompt: impl Fn(&str) -> String,
        fallback: impl FnOnce() -> String,
    ) -> (String, Option<StructuralUsage>) {
        let Some(model) = self.model.clone() else {
            return (fallback(), None);
        };
        let attempt = retry_dropping_oldest_rounds(strip_media_for_summarization(messages), |current| {
            let prompt = prompt(&serialize_conversation(current));
            let model = model.clone();
            async move {
                run_agent_completion(model, &prompt)
                    .await
                    .map(|(summary, usage)| (summary, Some(usage)))
            }
        })
        .await;
        summary_or_fallback(attempt, fallback)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::{MockAdapter, ModelAdapter, ModelCompletionRequest, ModelStreamEvent};
    use std::pin::Pin;
    use std::sync::Arc;

    #[tokio::test]
    async fn a_failed_summary_call_keeps_the_model_error_as_its_cause() {
        let mock = Arc::new(MockAdapter::default());
        mock.add_turn(vec![Err(AppError::Network("connection reset".to_string()))]);

        let Err(LlmCallError::Failed(cause)) = run_agent_completion(mock, "summarize").await else {
            panic!("the summary call should fail");
        };

        assert_eq!(cause.to_string(), "Network error: connection reset");
    }

    #[derive(Debug)]
    struct NeverAnswers;

    #[async_trait::async_trait]
    impl ModelAdapter for NeverAnswers {
        fn model_name(&self) -> &str {
            "never"
        }

        fn provider_name(&self) -> &str {
            "never"
        }

        async fn stream(
            &self,
            _request: ModelCompletionRequest,
        ) -> Result<Pin<Box<dyn futures::Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
            Ok(Box::pin(futures::stream::pending()))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_summary_call_past_the_deadline_fails_with_a_timeout_cause() {
        let model: ModelHandle = Arc::new(NeverAnswers);

        let Err(LlmCallError::Failed(cause)) = run_agent_completion(model, "summarize").await else {
            panic!("the summary call should time out");
        };

        assert_eq!(cause.to_string(), "Provider error: summary request timed out after 60s");
    }
}
