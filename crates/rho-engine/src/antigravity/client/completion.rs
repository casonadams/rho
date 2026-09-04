//! Rig `CompletionModel` adapter for [`AntigravityClient`]: the streaming-only
//! Cloud Code Assist surface aggregated into unary responses, plus the raw
//! event stream passthrough.

use super::AntigravityClient;
use super::http::{PROVIDER_NAME, friendly_error};
use crate::antigravity::stream::SseParser;
use futures::{StreamExt, stream};
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse, FinishReason, Usage};
use rig::message::{AssistantContent, Reasoning, ReasoningContent, Text, ToolCall};
use rig::streaming::{RawStreamingChoice, StreamFinal, StreamingCompletionResponse};

impl CompletionModel for AntigravityClient {
    async fn completion(&self, request: CompletionRequest) -> Result<CompletionResponse, CompletionError> {
        // The Cloud Code Assist surface is streaming-only; aggregate the SSE
        // stream into a single response (pi parity: no unary endpoint).
        let mut events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>> = Vec::new();
        self.feed_stream(&request, |batch| {
            events.extend(batch);
            Ok(())
        })
        .await?;
        aggregate_completion(events)
    }

    async fn stream(&self, request: CompletionRequest) -> Result<StreamingCompletionResponse, CompletionError> {
        let response = self
            .open_stream(&request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

        let event_stream = stream::unfold(
            (response.bytes_stream(), SseParser::new(), false),
            |(mut byte_stream, mut parser, finished)| async move {
                if finished {
                    return None;
                }
                loop {
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            let events = parser.feed(&bytes);
                            if !events.is_empty() {
                                let has_terminal = events
                                    .iter()
                                    .any(|event| matches!(event, Ok(RawStreamingChoice::FinalResponse(_)) | Err(_)));
                                return Some((events, (byte_stream, parser, has_terminal)));
                            }
                        }
                        Some(Err(e)) => {
                            let error = CompletionError::ProviderError(format!("Antigravity stream failed: {e}"));
                            return Some((vec![Err(error)], (byte_stream, parser, true)));
                        }
                        None => return None,
                    }
                }
            },
        )
        .map(stream::iter)
        .flatten();

        let boxed: std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<RawStreamingChoice<StreamFinal>, CompletionError>> + Send>,
        > = Box::pin(event_stream);
        Ok(StreamingCompletionResponse::stream(PROVIDER_NAME, boxed))
    }
}

fn aggregate_completion(
    events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>,
) -> Result<CompletionResponse, CompletionError> {
    let mut choice: Vec<AssistantContent> = Vec::new();
    let mut usage = Usage::new();
    let mut finish_reason: Option<FinishReason> = None;

    for event in events {
        match event {
            Err(error) => return Err(error),
            Ok(RawStreamingChoice::Message(text)) => {
                if let Some(AssistantContent::Text(last)) = choice.last_mut() {
                    last.text.push_str(&text);
                } else {
                    choice.push(AssistantContent::Text(Text::new(text)));
                }
            }
            Ok(RawStreamingChoice::Reasoning { content, .. }) => {
                choice.push(AssistantContent::Reasoning(Reasoning {
                    id: None,
                    content: vec![content],
                }));
            }
            Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) => {
                if let Some(AssistantContent::Reasoning(last)) = choice.last_mut()
                    && let Some(ReasoningContent::Text { text, .. }) = last.content.last_mut()
                {
                    text.push_str(&reasoning);
                } else {
                    choice.push(AssistantContent::Reasoning(Reasoning {
                        id: None,
                        content: vec![ReasoningContent::Text {
                            text: reasoning,
                            signature: None,
                        }],
                    }));
                }
            }
            Ok(RawStreamingChoice::ToolCall(call)) => {
                choice.push(AssistantContent::ToolCall(ToolCall::from(call)));
            }
            Ok(RawStreamingChoice::FinalResponse(final_response)) => {
                usage = final_response.usage;
                finish_reason = final_response.finish_reason;
            }
            Ok(_) => {}
        }
    }

    let mut response = CompletionResponse::new(choice, usage, PROVIDER_NAME);
    if let Some(finish_reason) = finish_reason {
        response = response.with_finish_reason(finish_reason);
    }
    Ok(response)
}
