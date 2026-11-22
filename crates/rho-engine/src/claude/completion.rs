//! `ModelAdapter` implementation for `ClaudeClient`.

use super::ClaudeClient;
use super::http::friendly_error;
use super::stream::SseParser;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelEventStream, ModelHandle};
use crate::provider::sse::open_sse_stream;
use rho_harness_core::error::AppError;
use std::sync::Arc;

#[async_trait::async_trait]
impl ModelAdapter for ClaudeClient {
    fn model_name(&self) -> &str {
        self.model()
    }

    fn provider_name(&self) -> &str {
        self.provider().as_str()
    }

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        open_sse_stream(self.open_stream(&req).await, friendly_error, SseParser::new())
    }

    fn for_short_lived_work(&self) -> Option<ModelHandle> {
        self.short_lived().map(|client| Arc::new(client) as ModelHandle)
    }

    fn cache_settings(&self) -> Option<&str> {
        self.thinking_level.as_deref()
    }
}
