//! Rig `CompletionModel` adapter for `ClaudeClient`.

use super::ClaudeClient;
use super::http::{PROVIDER_NAME, friendly_error};
use super::stream::SseParser;
use crate::provider::sse::{aggregate_stream_events, unfold_sse_stream};
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse};
use rig::streaming::{RawStreamingChoice, StreamFinal, StreamingCompletionResponse};

impl CompletionModel for ClaudeClient {
    async fn completion(&self, request: CompletionRequest) -> Result<CompletionResponse, CompletionError> {
        let mut events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>> = Vec::new();
        self.feed_stream(&request, |batch| {
            events.extend(batch);
            Ok(())
        })
        .await?;
        aggregate_stream_events(events, PROVIDER_NAME)
    }

    async fn stream(&self, request: CompletionRequest) -> Result<StreamingCompletionResponse, CompletionError> {
        let response = self
            .open_stream(&request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

        let stream = unfold_sse_stream(response, SseParser::new());
        Ok(StreamingCompletionResponse::stream(PROVIDER_NAME, stream))
    }
}
