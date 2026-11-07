//! `ModelAdapter` implementation for `ClaudeClient`.

use super::ClaudeClient;
use super::http::friendly_error;
use super::stream::SseParser;
use crate::engine::compactor::llm::ModelHandle;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelEventStream};
use crate::provider::sse::open_sse_stream;
use rho_harness_core::error::AppError;
use std::sync::Arc;

#[async_trait::async_trait]
impl ModelAdapter for ClaudeClient {
    fn model_name(&self) -> &str {
        self.model()
    }

    fn provider_name(&self) -> &str {
        "claude-code"
    }

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        open_sse_stream(self.open_stream(&req).await, friendly_error, SseParser::new())
    }
}

pub fn into_handle(client: ClaudeClient) -> ModelHandle {
    Arc::new(client)
}
