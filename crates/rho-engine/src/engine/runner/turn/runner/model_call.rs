use super::{AgentRunner, ModelRetry, MultiTurnStreamItem, StreamEvent, TurnEmitter};
use crate::engine::metrics::types::StructuralUsage;
use crate::provider::adapter::{FinishReason, ModelCompletionRequest, ModelStreamEvent};
use futures::{Stream, StreamExt};
use rho_harness_core::error::AppError;
use rho_harness_core::model::{AssistantContent, ToolCall};
use std::pin::Pin;

pub(super) const MODEL_CALL_ATTEMPTS: usize = 3;
const MODEL_RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub(super) fn retry_delay(attempt: usize, error: &AppError) -> std::time::Duration {
    let backoff = std::time::Duration::from_secs(1 << attempt.min(5));
    error
        .retry_after()
        .map_or(backoff, |wait| wait.min(crate::provider::error::MAX_RETRY_AFTER))
}

#[derive(Default)]
pub(super) struct TurnStreamOutcome {
    pub(super) text: String,
    pub(super) reasoning: String,
    pub(super) content: Vec<AssistantContent>,
    pub(super) tool_calls: Vec<ToolCall>,
    pub(super) usage: StructuralUsage,
    pub(super) finish_reason: Option<FinishReason>,
}

impl TurnStreamOutcome {
    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
        if let Some(AssistantContent::Text(last)) = self.content.last_mut() {
            last.text.push_str(text);
        } else {
            self.content.push(AssistantContent::text(text));
        }
    }
}

async fn stream_single_event(
    event: ModelStreamEvent,
    outcome: &mut TurnStreamOutcome,
    emitter: &TurnEmitter,
) -> Result<(), AppError> {
    match event {
        ModelStreamEvent::Text(t) => {
            outcome.push_text(&t);
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Text(t)))
                .await
        }
        ModelStreamEvent::Reasoning(r) => {
            outcome.reasoning.push_str(&r);
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Reasoning(r)))
                .await
        }
        ModelStreamEvent::ReasoningBlock(block) => {
            outcome.content.push(AssistantContent::Reasoning(block));
            Ok(())
        }
        ModelStreamEvent::ToolInputDelta { id, name, json } => {
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments {
                    id,
                    name,
                    json,
                }))
                .await
        }
        ModelStreamEvent::ToolCall(tc) => {
            emitter
                .send(MultiTurnStreamItem::StreamAssistantItem(StreamEvent::Arguments {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    json: String::new(),
                }))
                .await?;
            emitter
                .send(MultiTurnStreamItem::ToolCall { tool_call: tc.clone() })
                .await?;
            outcome.content.push(AssistantContent::ToolCall(tc.clone()));
            outcome.tool_calls.push(tc);
            Ok(())
        }
        ModelStreamEvent::Usage(u) => {
            outcome.usage = outcome.usage.merge(&u);
            Ok(())
        }
        ModelStreamEvent::Finish(f) => {
            outcome.finish_reason = Some(f);
            Ok(())
        }
    }
}

async fn stream_turn(
    mut stream: Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>,
    emitter: &TurnEmitter,
    outcome: &mut TurnStreamOutcome,
) -> Result<(), AppError> {
    while let Some(res) = stream.next().await {
        stream_single_event(res?, outcome, emitter).await?;
    }
    Ok(())
}

async fn attempt_model_call(
    runner: &AgentRunner,
    req: ModelCompletionRequest,
    emitter: &TurnEmitter,
    outcome: &mut TurnStreamOutcome,
) -> Result<(), AppError> {
    let stream = tokio::time::timeout(MODEL_RESPONSE_TIMEOUT, runner.model.stream(req))
        .await
        .map_err(|_| {
            AppError::Network(format!(
                "model provider sent no response within {}s",
                MODEL_RESPONSE_TIMEOUT.as_secs()
            ))
        })??;
    stream_turn(stream, emitter, outcome).await
}

/// Retries one model request on transient failures so a dropped connection costs a request, not the whole turn.
/// Each attempt builds its own request with `request`.
pub(super) async fn call_model(
    runner: &AgentRunner,
    request: impl Fn() -> ModelCompletionRequest,
    emitter: &TurnEmitter,
) -> Result<TurnStreamOutcome, AppError> {
    let mut attempt = 1;
    loop {
        let mut outcome = TurnStreamOutcome::default();
        let error = match attempt_model_call(runner, request(), emitter, &mut outcome).await {
            Ok(()) => return Ok(outcome),
            Err(error) if attempt < MODEL_CALL_ATTEMPTS && error.is_transient() => error,
            Err(error) => return Err(error),
        };
        let retry = ModelRetry {
            attempt,
            max_attempts: MODEL_CALL_ATTEMPTS,
            delay: retry_delay(attempt, &error),
            reason: error.to_string(),
            discarded_tool_calls: outcome.tool_calls.len(),
        };
        let delay = retry.delay;
        emitter.send(MultiTurnStreamItem::ModelTurnRetried(retry)).await?;
        runner.unless_cancelled(tokio::time::sleep(delay)).await?;
        attempt += 1;
    }
}
