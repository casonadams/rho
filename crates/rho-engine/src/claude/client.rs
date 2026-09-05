//! Claude Messages API client implementation.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::http::{DEFAULT_ENDPOINT, MESSAGES_PATH, PROVIDER_NAME, claude_headers, friendly_error, http_client};
use super::request::build_request_body;
use super::stream::SseParser;
use crate::auth::store::AuthStore;
use crate::auth::token::{AuthStoreTokenProvider, StaticTokenProvider, TokenProvider};
use futures::StreamExt;
use rig::agent::ModelHandle;
use rig::completion::{CompletionError, CompletionRequest};

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct ClaudeClient {
    token_provider: Arc<dyn TokenProvider>,
    model: String,
    thinking_level: Option<String>,
    endpoint: Option<String>,
}

impl ClaudeClient {
    pub fn new(token: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            token_provider: Arc::new(StaticTokenProvider::new(token)),
            model: model.into(),
            thinking_level: None,
            endpoint: None,
        }
    }

    pub fn with_token_provider(token_provider: Arc<dyn TokenProvider>, model: impl Into<String>) -> Self {
        Self {
            token_provider,
            model: model.into(),
            thinking_level: None,
            endpoint: None,
        }
    }

    pub fn with_auth_store(store: Arc<Mutex<AuthStore>>, model: impl Into<String>) -> Self {
        Self::with_token_provider(Arc::new(AuthStoreTokenProvider::new(store, "claude")), model)
    }

    pub fn with_thinking_level(mut self, level: Option<&str>) -> Self {
        self.thinking_level = level.map(String::from);
        self
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    fn target_url(&self) -> String {
        format!(
            "{}{}",
            self.endpoint.as_deref().unwrap_or(DEFAULT_ENDPOINT),
            MESSAGES_PATH
        )
    }

    async fn post_stream(
        &self,
        token: &str,
        request: &CompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let body = build_request_body(&self.model, self.thinking_level.as_deref(), request)
            .map_err(|e| (None, e.to_string()))?;
        let headers = claude_headers(token);
        let response = http_client()
            .post(self.target_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| (None, format!("Claude request failed: {e}")))?;

        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        Err((Some(status.as_u16()), text))
    }

    pub(crate) async fn open_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let mut token = self
            .token_provider
            .token()
            .await
            .map_err(|e| (None, format!("Failed to acquire Claude access token: {e}")))?;

        let mut res = self.post_stream(&token, request).await;
        if let Err((Some(401), ref body)) = res {
            if let Ok(new_token) = self.token_provider.force_refresh().await {
                token = new_token;
                res = self.post_stream(&token, request).await;
            } else {
                return Err((Some(401), body.clone()));
            }
        }
        res
    }

    pub(crate) async fn feed_stream(
        &self,
        request: &CompletionRequest,
        mut on_events: impl FnMut(super::stream::SseEvents) -> Result<(), CompletionError>,
    ) -> Result<(), CompletionError> {
        let response = self
            .open_stream(request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk.map_err(|e| CompletionError::ProviderError(format!("Claude stream failed: {e}")))?;
            on_events(parser.feed(&bytes))?;
        }
        Ok(())
    }
}

pub fn into_handle(client: ClaudeClient) -> ModelHandle {
    ModelHandle::named(PROVIDER_NAME, client)
}
