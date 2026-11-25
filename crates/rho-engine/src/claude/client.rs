//! Claude Messages API client implementation.

use std::sync::Arc;

use super::http::{ClaudeAuth, DEFAULT_ENDPOINT, MESSAGES_PATH, claude_headers};
use super::request::build_request_body;
use crate::auth::token::{StaticTokenProvider, TokenProvider};
use crate::http::post_json_checked;
use crate::provider::adapter::ModelCompletionRequest;
use crate::provider::error::{HttpFailure, HttpResult};
use rho_harness_core::config::CacheRetention;
use rho_harness_core::provider::ProviderId;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct ClaudeClient {
    token_provider: Arc<dyn TokenProvider>,
    auth: ClaudeAuth,
    pub(crate) model: String,
    pub(crate) thinking_level: Option<String>,
    pub(crate) cache_retention: CacheRetention,
    pub(crate) endpoint: Option<String>,
}

impl std::fmt::Debug for ClaudeClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ClaudeClient {{ model: {:?}, auth: {:?}, thinking_level: {:?} }}",
            self.model, self.auth, self.thinking_level
        )
    }
}

impl ClaudeClient {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn provider(&self) -> ProviderId {
        match self.auth {
            ClaudeAuth::Login => ProviderId::ClaudeCode,
            ClaudeAuth::ApiKey => ProviderId::Anthropic,
        }
    }
    pub fn new(token: impl Into<String>, model: impl Into<String>) -> Self {
        Self::with_token_provider(Arc::new(StaticTokenProvider::new(token)), model)
    }

    pub fn with_api_key(key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            auth: ClaudeAuth::ApiKey,
            ..Self::new(key, model)
        }
    }

    pub fn with_token_provider(token_provider: Arc<dyn TokenProvider>, model: impl Into<String>) -> Self {
        Self {
            token_provider,
            auth: ClaudeAuth::Login,
            model: model.into(),
            thinking_level: None,
            cache_retention: CacheRetention::default(),
            endpoint: None,
        }
    }

    pub fn with_thinking_level(mut self, level: Option<&str>) -> Self {
        self.thinking_level = level.map(String::from);
        self
    }

    pub fn with_cache_retention(mut self, retention: CacheRetention) -> Self {
        self.cache_retention = retention;
        self
    }

    /// A copy with short-lived cache retention, or `None` when the retention would not change.
    pub(crate) fn short_lived(&self) -> Option<Self> {
        let retention = self.cache_retention.for_short_lived_work();
        (retention != self.cache_retention).then(|| self.clone().with_cache_retention(retention))
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

    async fn post_stream(&self, token: &str, request: &ModelCompletionRequest) -> HttpResult {
        let effective_thinking = request
            .thinking_level
            .map(|level| level.concrete_or_default().as_str())
            .or(self.thinking_level.as_deref());
        let body = build_request_body(
            &self.model,
            effective_thinking,
            request,
            self.auth,
            self.cache_retention,
        )
        .map_err(|e| HttpFailure::Local(e.to_string()))?;
        post_json_checked(&self.target_url(), claude_headers(self.auth, token)?, &body).await
    }

    async fn refresh_and_retry(&self, request: &ModelCompletionRequest, body: &str) -> HttpResult {
        match self.token_provider.force_refresh().await {
            Ok(new_token) => self.post_stream(&new_token, request).await,
            Err(error) => Err(HttpFailure::Local(format!(
                "token refresh failed after 401 ({error}). Run 'rho login claude'. Backend: {body}"
            ))),
        }
    }

    pub(crate) async fn open_stream(&self, request: &ModelCompletionRequest) -> HttpResult {
        let token = self
            .token_provider
            .token()
            .await
            .map_err(|e| HttpFailure::Local(format!("Failed to acquire Claude access token: {e}")))?;

        match self.post_stream(&token, request).await {
            Err(failure) if failure.status_code() == Some(401) => self.refresh_and_retry(request, failure.body()).await,
            res => res,
        }
    }
}
