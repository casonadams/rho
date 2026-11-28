//! `ModelAdapter` implementation for `AntigravityClient`.

use super::AntigravityClient;
use super::http::friendly_error;
use crate::antigravity::stream::sse_parser;
use crate::provider::adapter::{ModelAdapter, ModelCompletionRequest, ModelEventStream};
use crate::provider::sse::open_sse_stream;
use rho_harness_core::error::AppError;

#[async_trait::async_trait]
impl ModelAdapter for AntigravityClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "antigravity"
    }

    async fn stream(&self, req: ModelCompletionRequest) -> Result<ModelEventStream, AppError> {
        open_sse_stream(self.open_stream(&req).await, friendly_error, sse_parser())
    }
}
