//! Claude Messages API client implementation.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::http::{ClaudeAuth, DEFAULT_ENDPOINT, MESSAGES_PATH, claude_headers};
use super::request::build_request_body;
use crate::auth::store::AuthStore;
use crate::auth::token::{AuthStoreTokenProvider, StaticTokenProvider, TokenProvider};
use crate::http::post_json_checked;
use crate::provider::adapter::ModelCompletionRequest;

#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct ClaudeClient {
    token_provider: Arc<dyn TokenProvider>,
    auth: ClaudeAuth,
    pub(crate) model: String,
    pub(crate) thinking_level: Option<String>,
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
        request: &ModelCompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let body = build_request_body(&self.model, self.thinking_level.as_deref(), request, self.auth)
            .map_err(|e| (None, e.to_string()))?;
        post_json_checked(&self.target_url(), claude_headers(self.auth, token), &body, None).await
    }

    async fn refresh_and_retry(
        &self,
        request: &ModelCompletionRequest,
        body: &str,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        match self.token_provider.force_refresh().await {
            Ok(new_token) => self.post_stream(&new_token, request).await,
            Err(error) => Err((
                None,
                format!("token refresh failed after 401 ({error}). Run 'rho login claude'. Backend: {body}"),
            )),
        }
    }

    pub(crate) async fn open_stream(
        &self,
        request: &ModelCompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let token = self
            .token_provider
            .token()
            .await
            .map_err(|e| (None, format!("Failed to acquire Claude access token: {e}")))?;

        let res = self.post_stream(&token, request).await;
        if let Err((Some(401), ref body)) = res {
            self.refresh_and_retry(request, body).await
        } else {
            res
        }
    }
}

pub use super::completion::into_handle;
