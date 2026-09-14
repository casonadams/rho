//! Rig `CompletionModel` adapter for [`AntigravityClient`]: the streaming-only
//! Cloud Code Assist surface aggregated into unary responses, plus the raw
//! event stream passthrough.

use super::AntigravityClient;
use super::http::{PROVIDER_NAME, friendly_error};
use crate::antigravity::stream::SseParser;
use crate::provider::sse::{aggregate_stream_events, unfold_sse_stream};
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse};
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
