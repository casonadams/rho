//! `ModelAdapter` implementation for `ClaudeClient`.

use super::ClaudeClient;
use super::http::friendly_error;
use super::stream::unfold_claude_stream;
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelCompletionResponse, ModelStreamEvent};
use futures::Stream;
use rho_harness_core::error::AppError;
use std::pin::Pin;
use std::sync::Arc;

#[async_trait::async_trait]
impl ModelAdapter for ClaudeClient {
    fn model_name(&self) -> &str {
        self.model()
    }

    fn provider_name(&self) -> &str {
        "claude-code"
    }

    async fn complete(&self, req: ModelCompletionRequest) -> Result<ModelCompletionResponse, AppError> {
        let stream = self.stream(req).await?;
        let events = futures::StreamExt::collect::<Vec<_>>(stream).await;
        crate::provider::sse::aggregate_stream_events(events)
    }

    async fn stream(
        &self,
        req: ModelCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ModelStreamEvent, AppError>> + Send>>, AppError> {
        let response = self
            .open_stream(&req)
            .await
            .map_err(|(status, body)| AppError::Provider(friendly_error(status, &body)))?;

        Ok(Box::pin(unfold_claude_stream(response)))
    }
}

pub fn into_handle(client: ClaudeClient) -> ModelHandle {
    Arc::new(client)
}
