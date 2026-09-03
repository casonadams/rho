//! Antigravity HTTP client: endpoints, headers, project/model discovery, and
//! the rig `CompletionModel` implementation over `streamGenerateContent`.

use super::request::{self, Effort, RequestTarget};
use super::stream::SseParser;
use futures::{StreamExt, stream};
use reqwest::header::{HeaderMap, HeaderValue};
use rig::agent::ModelHandle;
use rig::completion::{CompletionError, CompletionModel, CompletionRequest, CompletionResponse, FinishReason, Usage};
use rig::message::{AssistantContent, Reasoning, ReasoningContent, Text, ToolCall};
use rig::streaming::{RawStreamingChoice, StreamFinal, StreamingCompletionResponse};
use std::sync::LazyLock;
use std::time::Duration;

pub const DEFAULT_ENDPOINT: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const ENDPOINT_CANDIDATES: [&str; 3] = [
    DEFAULT_ENDPOINT,
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(8);
const PROVIDER_NAME: &str = "antigravity";

static HTTP_CLIENT: LazyLock<reqwest::Client> =
    LazyLock::new(|| reqwest::Client::builder().no_proxy().build().unwrap_or_default());

pub fn http_client() -> &'static reqwest::Client {
    &HTTP_CLIENT
}

/// Headers Cloud Code Assist expects on every call (pi-antigravity parity).
pub fn antigravity_headers(token: &str) -> HeaderMap {
    let platform = match std::env::consts::OS {
        "macos" => "MACOS",
        "windows" => "WINDOWS",
        _ => "LINUX",
    };
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}")) {
        headers.insert("Authorization", value);
    }
    headers.insert("Content-Type", HeaderValue::from_static("application/json"));
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("antigravity/hub/2.8.0 (aidev_client; os_type=darwin; arch=arm64; cl=963137146)"),
    );
    headers.insert(
        "X-Goog-Api-Client",
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );
    if let Ok(metadata) = HeaderValue::from_str(&format!(
        r#"{{"ideType":"ANTIGRAVITY","platform":"{platform}","pluginType":"GEMINI"}}"#
    )) {
        headers.insert("Client-Metadata", metadata);
    }
    headers
}

fn friendly_error(status: Option<u16>, body: &str) -> String {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| body.chars().take(300).collect());
    match status {
        Some(429) if message.contains("Individual quota reached") => {
            let reset = message
                .split("Resets in ")
                .nth(1)
                .map(|r| r.trim_end_matches('.'))
                .unwrap_or("unknown");
            format!("Antigravity quota reached. Resets in {reset}. Switch models or wait for the reset.")
        }
        Some(429) => "Antigravity rate limit reached. Wait a bit and retry.".to_string(),
        Some(401) => "Antigravity login expired or credentials are invalid. Run 'rho login antigravity'.".to_string(),
        Some(403) => format!("Antigravity access denied. Re-login or try another model. Backend: {message}"),
        Some(404) => format!("Model not available on Antigravity. Backend: {message}"),
        Some(503) if message.contains("No capacity") => {
            "This model has no capacity right now. Try another model.".to_string()
        }
        Some(other) => format!("Antigravity API error ({other}): {message}"),
        None => format!("Antigravity request failed: {message}"),
    }
}

/// POST a Cloud Code Assist metadata endpoint, trying endpoint candidates.
async fn post_metadata(path: &str, token: &str, body: serde_json::Value) -> Option<serde_json::Value> {
    for endpoint in ENDPOINT_CANDIDATES {
        let response = http_client()
            .post(format!("{endpoint}{path}"))
            .headers(antigravity_headers(token))
            .json(&body)
            .timeout(DISCOVERY_TIMEOUT)
            .send()
            .await;
        if let Ok(response) = response
            && response.status().is_success()
            && let Ok(json) = response.json::<serde_json::Value>().await
        {
            return Some(json);
        }
    }
    None
}

fn extract_project_id(value: &serde_json::Value) -> Option<String> {
    let direct = value
        .get("antigravityProjectId")
        .or_else(|| value.get("projectId"))
        .or_else(|| value.get("backendProjectId"))
        .or_else(|| value.get("cloudaicompanionProject"));
    if let Some(id) = direct.and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }
    for key in ["projects", "projectIds", "cloudaicompanionProjects"] {
        if let Some(items) = value.get(key).and_then(|v| v.as_array()) {
            for item in items {
                if let Some(id) = item.as_str() {
                    return Some(id.to_string());
                }
                if let Some(found) = extract_project_id(item) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Discover the Cloud Code Assist project id for the signed-in account.
pub async fn load_project_id(token: &str) -> Option<String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    if let Some(project) = post_metadata("/v1internal:loadCodeAssist", token, body)
        .await
        .as_ref()
        .and_then(extract_project_id)
    {
        return Some(project);
    }
    post_metadata("/v1internal:listCloudAICompanionProjects", token, serde_json::json!({}))
        .await
        .as_ref()
        .and_then(extract_project_id)
}

/// Runtime models selectable in rho (pi parity filters: gemini-/claude-/
/// gpt-oss- prefixed, no chat/tab/image entries).
pub fn is_selectable_runtime_model(id: &str) -> bool {
    let selectable = id.starts_with("gemini-") || id.starts_with("claude-") || id.starts_with("gpt-oss-");
    selectable && !id.contains(char::is_whitespace) && !id.contains("image") && !id.starts_with("MODEL_")
}

/// Live model catalog via `v1internal:fetchAvailableModels`.
pub async fn discover_models(token: &str, project_id: &str) -> Option<Vec<String>> {
    let response = post_metadata(
        "/v1internal:fetchAvailableModels",
        token,
        serde_json::json!({ "project": project_id }),
    )
    .await?;
    let models = response.get("models")?.as_object()?;
    let mut ids: Vec<String> = models
        .keys()
        .filter(|id| is_selectable_runtime_model(id))
        .cloned()
        .collect();
    if ids.is_empty() {
        return None;
    }
    ids.sort();
    Some(ids)
}

/// One (endpoint, project, runtime-model) routing combination for a stream POST.
#[derive(Clone, Copy)]
pub struct Endpoint<'a> {
    base_url: &'a str,
    project: &'a str,
    runtime_model: &'a str,
    effort: Effort,
}

impl Endpoint<'_> {
    fn wire_target(&self) -> RequestTarget<'_> {
        RequestTarget {
            project: self.project,
            runtime_model: self.runtime_model,
            effort: self.effort,
        }
    }
}

/// Rig client for the Antigravity Cloud Code Assist API.
#[derive(Clone)]
pub struct AntigravityClient {
    token: String,
    project_id: String,
    model: String,
    effort: Effort,
}

impl AntigravityClient {
    pub fn new(token: impl Into<String>, project_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            project_id: project_id.into(),
            model: model.into(),
            effort: Effort::Off,
        }
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
        endpoint: Endpoint<'_>,
        request: &CompletionRequest,
    ) -> Result<reqwest::Response, (Option<u16>, String)> {
        let envelope = request::new_envelope();
        let target = endpoint.wire_target();
        let body = request::build_request_body(target, request, &envelope).map_err(|e| (None, e.to_string()))?;
        let mut headers = antigravity_headers(&self.token);
        if request::wants_claude_thinking_header(target.runtime_model, target.effort) {
            headers.insert(
                "anthropic-beta",
                reqwest::header::HeaderValue::from_static("interleaved-thinking-2025-05-14"),
            );
        }
        let response = http_client()
            .post(Self::streaming_endpoint(endpoint.base_url))
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
        let runtime_model = request::resolve_runtime_model(&self.model, self.effort);
        let mut candidates = vec![runtime_model.clone()];
        if let Some(fallback) = request::fallback_runtime_model(&runtime_model) {
            candidates.push(fallback);
        }

        let mut last: Option<(Option<u16>, String)> = None;
        for candidate in candidates {
            for candidate_endpoint in ENDPOINT_CANDIDATES {
                let endpoint = Endpoint {
                    base_url: candidate_endpoint,
                    project: &self.project_id,
                    runtime_model: &candidate,
                    effort: self.effort,
                };
                match self.post_stream(endpoint, request).await {
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
        aggregate_completion(events)
    }

    async fn stream(&self, request: CompletionRequest) -> Result<StreamingCompletionResponse, CompletionError> {
        let response = self
            .open_stream(&request)
            .await
            .map_err(|(status, body)| CompletionError::ProviderError(friendly_error(status, &body)))?;

        let event_stream = stream::unfold(
            (response.bytes_stream(), SseParser::new(), false),
            |(mut byte_stream, mut parser, finished)| async move {
                if finished {
                    return None;
                }
                loop {
                    match byte_stream.next().await {
                        Some(Ok(bytes)) => {
                            let events = parser.feed(&bytes);
                            if !events.is_empty() {
                                let has_terminal = events
                                    .iter()
                                    .any(|event| matches!(event, Ok(RawStreamingChoice::FinalResponse(_)) | Err(_)));
                                return Some((events, (byte_stream, parser, has_terminal)));
                            }
                        }
                        Some(Err(e)) => {
                            let error = CompletionError::ProviderError(format!("Antigravity stream failed: {e}"));
                            return Some((vec![Err(error)], (byte_stream, parser, true)));
                        }
                        None => return None,
                    }
                }
            },
        )
        .map(stream::iter)
        .flatten();

        let boxed: std::pin::Pin<
            Box<dyn futures::Stream<Item = Result<RawStreamingChoice<StreamFinal>, CompletionError>> + Send>,
        > = Box::pin(event_stream);
        Ok(StreamingCompletionResponse::stream(PROVIDER_NAME, boxed))
    }
}

fn aggregate_completion(
    events: Vec<Result<RawStreamingChoice<StreamFinal>, CompletionError>>,
) -> Result<CompletionResponse, CompletionError> {
    let mut choice: Vec<AssistantContent> = Vec::new();
    let mut usage = Usage::new();
    let mut finish_reason: Option<FinishReason> = None;

    for event in events {
        match event {
            Err(error) => return Err(error),
            Ok(RawStreamingChoice::Message(text)) => {
                if let Some(AssistantContent::Text(last)) = choice.last_mut() {
                    last.text.push_str(&text);
                } else {
                    choice.push(AssistantContent::Text(Text::new(text)));
                }
            }
            Ok(RawStreamingChoice::Reasoning { content, .. }) => {
                choice.push(AssistantContent::Reasoning(Reasoning {
                    id: None,
                    content: vec![content],
                }));
            }
            Ok(RawStreamingChoice::ReasoningDelta { reasoning, .. }) => {
                if let Some(AssistantContent::Reasoning(last)) = choice.last_mut()
                    && let Some(ReasoningContent::Text { text, .. }) = last.content.last_mut()
                {
                    text.push_str(&reasoning);
                } else {
                    choice.push(AssistantContent::Reasoning(Reasoning {
                        id: None,
                        content: vec![ReasoningContent::Text {
                            text: reasoning,
                            signature: None,
                        }],
                    }));
                }
            }
            Ok(RawStreamingChoice::ToolCall(call)) => {
                choice.push(AssistantContent::ToolCall(ToolCall::from(call)));
            }
            Ok(RawStreamingChoice::FinalResponse(final_response)) => {
                usage = final_response.usage;
                finish_reason = final_response.finish_reason;
            }
            Ok(_) => {}
        }
    }

    let mut response = CompletionResponse::new(choice, usage, PROVIDER_NAME);
    if let Some(finish_reason) = finish_reason {
        response = response.with_finish_reason(finish_reason);
    }
    Ok(response)
}

/// Wrap into a rig model handle for the engine.
pub fn into_handle(client: AntigravityClient) -> ModelHandle {
    ModelHandle::named(PROVIDER_NAME, client)
}
