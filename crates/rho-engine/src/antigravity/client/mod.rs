//! Antigravity streaming client: endpoint/model fallback routing and the SSE
//! feed consumed by the rig [`CompletionModel`] adapter in `completion`.

use self::http::{PROVIDER_NAME, friendly_error};
use super::request::{self, Effort, RequestTarget};
use super::stream::SseParser;
use futures::StreamExt;
use rig::agent::ModelHandle;
use rig::completion::{CompletionError, CompletionRequest};
use rig::streaming::{RawStreamingChoice, StreamFinal};

pub mod completion;
pub mod discovery;
pub mod http;
pub mod token;

#[cfg(test)]
mod tests;

pub use discovery::{discover_models, is_selectable_runtime_model, load_project_id};
pub(crate) use http::post_metadata;
pub use http::{DEFAULT_ENDPOINT, ENDPOINT_CANDIDATES, antigravity_headers, http_client};
pub use token::{AuthStoreTokenProvider, StaticTokenProvider, TokenProvider};

/// One (endpoint, project, runtime-model) routing combination for a stream POST.
#[derive(Clone, Copy)]
pub struct Endpoint<'a> {
    base_url: &'a str,
    project: &'a str,
    runtime_model: &'a str,
    effort: Effort,
}

#[derive(Clone, Copy)]
struct EndpointTarget<'a, 't> {
    endpoint: Endpoint<'a>,
    token: &'t str,
}

impl<'a> Endpoint<'a> {
    fn wire_target(&self) -> RequestTarget<'_> {
        RequestTarget {
            project: self.project,
            runtime_model: self.runtime_model,
            effort: self.effort,
        }
    }

    fn with_token<'t>(&self, token: &'t str) -> EndpointTarget<'a, 't> {
        EndpointTarget { endpoint: *self, token }
    }
}

/// Rig client for the Antigravity Cloud Code Assist API.
#[derive(Clone)]
pub struct AntigravityClient {
    token_provider: std::sync::Arc<dyn TokenProvider>,
    project_id: String,
    model: String,
    effort: Effort,
    endpoint: Option<String>,
}

impl AntigravityClient {
    pub fn new(token: impl Into<String>, project_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            token_provider: std::sync::Arc::new(StaticTokenProvider::new(token)),
            project_id: project_id.into(),
            model: model.into(),
            effort: Effort::Off,
            endpoint: None,
        }
    }

    pub fn with_token_provider(
        token_provider: std::sync::Arc<dyn TokenProvider>,
        project_id: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            token_provider,
            project_id: project_id.into(),
            model: model.into(),
            effort: Effort::Off,
            endpoint: None,
        }
    }

    pub fn with_auth_store(
        store: std::sync::Arc<tokio::sync::Mutex<crate::auth::store::AuthStore>>,
        project_id: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self::with_token_provider(
            std::sync::Arc::new(AuthStoreTokenProvider::new(store, "antigravity")),
            project_id,
            model,
        )
    }

    /// Override the backend API endpoint base URL (primarily for tests and custom proxies).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the thinking effort (rho's `thinking_level`) used to pick the
    /// backend runtime variant and thinking config.
    pub fn with_effort(mut self, level: Option<&str>) -> Self {
        self.effort = Effort::parse(level);
        self
    }

    fn streaming_endpoint(endpoint: &str) -> String {
        format!("{endpoint}/v1internal:streamGenerateContent?alt=sse")
    }

    async fn post_stream(
        &self,
        target: EndpointTarget<'_, '_>,
        request: &CompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let envelope = request::new_envelope();
        let wire_target = target.endpoint.wire_target();
        let body = request::build_request_body(wire_target, request, &envelope).map_err(|e| (None, e.to_string()))?;
        let mut headers = antigravity_headers(target.token);
        if request::wants_claude_thinking_header(wire_target.runtime_model, wire_target.effort) {
            headers.insert(
                "anthropic-beta",
                reqwest::header::HeaderValue::from_static("interleaved-thinking-2025-05-14"),
            );
        }
        let response = http_client()
            .post(Self::streaming_endpoint(target.endpoint.base_url))
            .headers(headers)
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| (None, format!("Antigravity request failed: {e}")))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        Err((Some(status.as_u16()), text))
    }

    async fn open_stream(&self, request: &CompletionRequest) -> Result<reqwest::Response, (Option<u16>, String)> {
        let mut token = self
            .token_provider
            .token()
            .await
            .map_err(|e| (None, format!("Failed to acquire Antigravity access token: {e}")))?;
        let runtime_model = request::resolve_runtime_model(&self.model, self.effort);
        let mut candidates = vec![runtime_model.clone()];
        if let Some(fallback) = request::fallback_runtime_model(&runtime_model) {
            candidates.push(fallback);
        }

        let mut last: Option<(Option<u16>, String)> = None;
        let mut refreshed = false;
        let endpoints: Vec<&str> = match self.endpoint.as_deref() {
            Some(custom) => vec![custom],
            None => ENDPOINT_CANDIDATES.to_vec(),
        };
        for candidate in candidates {
            for &candidate_endpoint in &endpoints {
                let endpoint = Endpoint {
                    base_url: candidate_endpoint,
                    project: &self.project_id,
                    runtime_model: &candidate,
                    effort: self.effort,
                };
                let mut res = self.post_stream(endpoint.with_token(&token), request).await;
                if let Err((Some(401), ref body)) = res
                    && !refreshed
                {
                    if let Ok(new_token) = self.token_provider.force_refresh().await {
                        token = new_token;
                        refreshed = true;
                        res = self.post_stream(endpoint.with_token(&token), request).await;
                    } else {
                        return Err((Some(401), body.clone()));
                    }
                }
                match res {
                    Ok(response) => return Ok(response),
                    Err((Some(429), body)) if body.contains("Individual quota reached") => {
                        // Quota is account-wide; other endpoints won't help.
                        return Err((Some(429), body));
                    }
                    Err((Some(status), body)) if [403, 404, 429, 500, 502, 503, 504].contains(&status) => {
                        last = Some((Some(status), body));
                    }
                    Err(other) => return Err(other),
                }
            }
        }
        Err(last.unwrap_or((None, "no endpoint available".to_string())))
    }

    async fn feed_stream(
        &self,
        request: &CompletionRequest,
        mut on_events: impl FnMut(
            Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>,
        ) -> Result<(), CompletionError>,
    ) -> Result<(), CompletionError> {
        let response = self
            .open_stream(request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;
        let mut parser = SseParser::new();
        let mut byte_stream = response.bytes_stream();
        while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk.map_err(|e| CompletionError::ProviderError(format!("Antigravity stream failed: {e}")))?;
            on_events(parser.feed(&bytes))?;
        }
        Ok(())
    }
}

/// Wrap into a rig model handle for the engine.
pub fn into_handle(client: AntigravityClient) -> ModelHandle {
    ModelHandle::named(PROVIDER_NAME, client)
}
