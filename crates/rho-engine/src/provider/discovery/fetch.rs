//! Provider model-list endpoints and their response parsing.

use super::DiscoveredModel;
use super::antigravity::{collapse_antigravity_catalog, sort_models_newest_first};
use super::presets::{antigravity_preset_models, format_context_tokens};
use crate::claude::http::{ClaudeAuth, claude_headers};
use crate::http::{CLIENT, get_json};
use rho_harness_core::error::{AppError, Result};
use serde_json::Value;
use std::time::Duration;

const LIST_TIMEOUT: Duration = Duration::from_secs(15);
const CONTEXT_FIELDS: &[&str] = &[
    "context_window",
    "context_length",
    "max_context_length",
    "max_input_tokens",
    "inputTokenLimit",
];

pub(crate) const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";
pub(crate) const CODEX_MODELS_URL: &str = "https://chatgpt.com/backend-api/codex/models";
/// The Codex catalog only lists models for clients at or above a version; 0.22.4 receives none.
pub(crate) const CODEX_CLIENT_VERSION: &str = "0.171.0";
pub(crate) const GEMINI_MODELS_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000";

fn context_of(entry: &Value) -> Option<usize> {
    CONTEXT_FIELDS
        .iter()
        .find_map(|field| entry.get(field)?.as_u64())
        .or_else(|| entry.pointer("/top_provider/context_length")?.as_u64())
        .and_then(|tokens| usize::try_from(tokens).ok())
}

fn str_field<'a>(entry: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| entry.get(key)?.as_str())
}

fn listed_model(entry: &Value, id: &str, provider: &str) -> DiscoveredModel {
    let context_tokens = context_of(entry);
    DiscoveredModel {
        id: id.to_string(),
        name: str_field(entry, &["display_name", "displayName", "name"])
            .unwrap_or(id)
            .to_string(),
        provider: provider.to_string(),
        description: context_tokens.map(format_context_tokens).unwrap_or_default(),
        context_tokens,
    }
}

fn entries<'a>(body: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    body.get(key).and_then(Value::as_array).into_iter().flatten()
}

/// OpenAI-style `data[].id` lists (also Anthropic's) and Cohere-style `models[].name` lists.
pub(crate) fn parse_model_list(body: &Value, provider: &str) -> Vec<DiscoveredModel> {
    entries(body, "data")
        .chain(entries(body, "models"))
        .filter_map(|entry| Some(listed_model(entry, str_field(entry, &["id", "name"])?, provider)))
        .collect()
}

pub(crate) fn parse_codex_models(body: &Value) -> Vec<DiscoveredModel> {
    entries(body, "models")
        .filter(|entry| str_field(entry, &["visibility"]).is_none_or(|v| v == "list"))
        .filter_map(|entry| Some(listed_model(entry, str_field(entry, &["slug"])?, "chatgpt")))
        .collect()
}

pub(crate) fn parse_gemini_models(body: &Value) -> Vec<DiscoveredModel> {
    let generates = |entry: &&Value| {
        entries(entry, "supportedGenerationMethods").any(|method| method.as_str() == Some("generateContent"))
    };
    entries(body, "models")
        .filter(generates)
        .filter_map(|entry| {
            let name = str_field(entry, &["name"])?;
            Some(listed_model(
                entry,
                name.strip_prefix("models/").unwrap_or(name),
                "gemini",
            ))
        })
        .collect()
}

fn list_request(url: &str) -> reqwest::RequestBuilder {
    CLIENT.get(url).timeout(LIST_TIMEOUT)
}

fn label(provider: &str) -> String {
    format!("{provider} model list")
}

pub(crate) async fn fetch_openai_compatible(
    url: &str,
    key: Option<&str>,
    provider: &str,
) -> Result<Vec<DiscoveredModel>> {
    let mut request = list_request(url);
    if let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) {
        request = request.bearer_auth(key);
    }
    Ok(parse_model_list(&get_json(request, &label(provider)).await?, provider))
}

pub(crate) async fn fetch_anthropic(
    url: &str,
    auth: ClaudeAuth,
    token: &str,
    provider: &str,
) -> Result<Vec<DiscoveredModel>> {
    let request = list_request(url).headers(claude_headers(auth, token));
    Ok(parse_model_list(&get_json(request, &label(provider)).await?, provider))
}

pub(crate) async fn fetch_codex(url: &str, token: &str, account_id: Option<&str>) -> Result<Vec<DiscoveredModel>> {
    let mut request = list_request(&format!("{url}?client_version={CODEX_CLIENT_VERSION}"))
        .bearer_auth(token)
        .header("originator", "codex")
        .header(reqwest::header::USER_AGENT, format!("Codex/{CODEX_CLIENT_VERSION}"));
    if let Some(account_id) = account_id {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    let models = parse_codex_models(&get_json(request, &label("chatgpt")).await?);
    if models.is_empty() {
        return Err(AppError::Provider(format!(
            "chatgpt model list is empty for Codex client version {CODEX_CLIENT_VERSION}"
        )));
    }
    Ok(models)
}

pub(crate) async fn fetch_gemini(url: &str, key: &str) -> Result<Vec<DiscoveredModel>> {
    let request = list_request(url).header("x-goog-api-key", key);
    Ok(parse_gemini_models(&get_json(request, &label("gemini")).await?))
}

pub(crate) async fn fetch_ollama(host: &str) -> Result<Vec<DiscoveredModel>> {
    let request = list_request(&format!("{host}/v1/models"));
    let mut models = parse_model_list(&get_json(request, "Ollama /v1/models").await?, "local");
    let windows = futures::future::join_all(
        models
            .iter()
            .map(|m| crate::ollama::local::show_context_length(host, &m.id)),
    )
    .await;
    for (model, window) in models.iter_mut().zip(windows) {
        if let Some(tokens) = window {
            model.description = format_context_tokens(tokens);
            model.context_tokens = Some(tokens);
        }
    }
    Ok(models)
}

pub(crate) async fn fetch_antigravity(token: &str, project_id: &str) -> Vec<DiscoveredModel> {
    let presets = antigravity_preset_models();
    let Some(ids) = crate::antigravity::discover_models(token, project_id).await else {
        return presets;
    };
    let mut discovered = collapse_antigravity_catalog(ids);
    for preset in presets {
        if !discovered.iter().any(|m| m.id == preset.id) {
            discovered.push(preset);
        }
    }
    sort_models_newest_first(discovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{serve_http, serve_http_recording};
    use serde_json::json;

    fn summary(models: &[DiscoveredModel]) -> Vec<(&str, &str, Option<usize>)> {
        models
            .iter()
            .map(|m| (m.id.as_str(), m.name.as_str(), m.context_tokens))
            .collect()
    }

    #[test]
    fn model_lists_read_each_documented_context_field() {
        let body = json!({ "data": [
            { "id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5", "max_input_tokens": 1_000_000 },
            { "id": "llama-3.3-70b", "context_window": 131_072 },
            { "id": "deepseek-chat", "name": "DeepSeek Chat", "context_window": 131_072 },
            { "id": "anthropic/claude-x", "context_length": 200_000 },
            { "id": "mistral-large", "max_context_length": 128_000 },
            { "id": "or/model", "top_provider": { "context_length": 64_000 } },
            { "id": "gpt-6" },
            { "object": "model" }
        ] });
        assert_eq!(
            summary(&parse_model_list(&body, "x")),
            vec![
                ("claude-sonnet-5-5", "Claude Sonnet 5.5", Some(1_000_000)),
                ("llama-3.3-70b", "llama-3.3-70b", Some(131_072)),
                ("deepseek-chat", "DeepSeek Chat", Some(131_072)),
                ("anthropic/claude-x", "anthropic/claude-x", Some(200_000)),
                ("mistral-large", "mistral-large", Some(128_000)),
                ("or/model", "or/model", Some(64_000)),
                ("gpt-6", "gpt-6", None),
            ]
        );
        let cohere = json!({ "models": [{ "name": "command-a", "context_length": 256_000 }] });
        assert_eq!(
            summary(&parse_model_list(&cohere, "cohere")),
            vec![("command-a", "command-a", Some(256_000))]
        );
    }

    #[test]
    fn codex_list_keeps_only_listed_models() {
        let body = json!({ "models": [
            { "slug": "gpt-reserve", "display_name": "GPT-Reserve", "visibility": "hide", "context_window": 272_000 },
            { "slug": "gpt-6-luna", "display_name": "GPT-6-Luna", "visibility": "list", "context_window": 272_000 },
            { "slug": "gpt-5.6-terra", "context_window": 272_000 }
        ] });
        assert_eq!(
            summary(&parse_codex_models(&body)),
            vec![
                ("gpt-6-luna", "GPT-6-Luna", Some(272_000)),
                ("gpt-5.6-terra", "gpt-5.6-terra", Some(272_000)),
            ]
        );
    }

    #[test]
    fn gemini_list_keeps_generate_content_models_without_prefix() {
        let body = json!({ "models": [
            {
                "name": "models/gemini-2.5-pro",
                "displayName": "Gemini 2.5 Pro",
                "inputTokenLimit": 1_048_576,
                "supportedGenerationMethods": ["generateContent", "countTokens"]
            },
            { "name": "models/text-embedding-004", "inputTokenLimit": 2048, "supportedGenerationMethods": ["embedContent"] }
        ] });
        assert_eq!(
            summary(&parse_gemini_models(&body)),
            vec![("gemini-2.5-pro", "Gemini 2.5 Pro", Some(1_048_576))]
        );
    }

    #[tokio::test]
    async fn fetchers_send_provider_credentials() {
        let anthropic = json!({ "data": [{ "id": "claude-opus-5", "max_input_tokens": 1_000_000 }] }).to_string();
        let codex = json!({ "models": [{ "slug": "gpt-6-luna", "visibility": "list" }] }).to_string();
        let gemini = json!({ "models": [] }).to_string();
        let openai = json!({ "data": [] }).to_string();
        let (base, requests) = serve_http_recording(vec![
            ("200 OK", anthropic),
            ("200 OK", codex),
            ("200 OK", gemini),
            ("200 OK", openai),
        ])
        .await;

        let claude = fetch_anthropic(&format!("{base}/v1/models"), ClaudeAuth::Login, "oauth-tok", "claude")
            .await
            .unwrap();
        assert_eq!(claude[0].context_tokens, Some(1_000_000));
        fetch_codex(&format!("{base}/codex/models"), "codex-tok", Some("acct-1"))
            .await
            .unwrap();
        fetch_gemini(&format!("{base}/v1beta/models"), "gem-key").await.unwrap();
        fetch_openai_compatible(&format!("{base}/v1/models"), Some(" sk-1 "), "openai")
            .await
            .unwrap();

        let requests = requests.lock().unwrap();
        assert!(
            requests[0].contains("authorization: bearer oauth-tok"),
            "{}",
            requests[0]
        );
        assert!(requests[0].contains("anthropic-beta: "), "{}", requests[0]);
        assert!(requests[1].starts_with(&format!("get /codex/models?client_version={CODEX_CLIENT_VERSION} ")));
        assert!(requests[1].contains("chatgpt-account-id: acct-1"), "{}", requests[1]);
        assert!(requests[2].contains("x-goog-api-key: gem-key"), "{}", requests[2]);
        assert!(
            requests[3].contains("authorization: bearer sk-1\r\n"),
            "{}",
            requests[3]
        );
    }

    #[tokio::test]
    async fn fetch_failures_and_empty_codex_catalog_are_errors() {
        let base = serve_http(vec![
            (
                "401 Unauthorized",
                r#"{"error":{"message":"invalid x-api-key"}}"#.to_string(),
            ),
            ("200 OK", r#"{"models":[]}"#.to_string()),
        ])
        .await;
        let denied = fetch_anthropic(&base, ClaudeAuth::ApiKey, "bad", "anthropic")
            .await
            .unwrap_err()
            .to_string();
        assert!(
            denied.contains("returned HTTP 401") && denied.contains("invalid x-api-key"),
            "{denied}"
        );
        let empty = fetch_codex(&base, "tok", None).await.unwrap_err().to_string();
        assert!(empty.contains("model list is empty"), "{empty}");
    }

    #[tokio::test]
    async fn ollama_list_includes_each_models_context_window() {
        let host = serve_http(vec![
            ("200 OK", json!({ "data": [{ "id": "qwen2.5-coder:7b" }] }).to_string()),
            (
                "200 OK",
                json!({ "model_info": { "qwen2.context_length": 32_768 } }).to_string(),
            ),
        ])
        .await;
        let models = fetch_ollama(&host).await.unwrap();
        assert_eq!(
            summary(&models),
            vec![("qwen2.5-coder:7b", "qwen2.5-coder:7b", Some(32_768))]
        );
        assert_eq!(models[0].description, "32k ctx");
    }
}
