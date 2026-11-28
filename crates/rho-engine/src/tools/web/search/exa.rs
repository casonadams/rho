use std::path::Path;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::auth::AuthStore;
use crate::tools::web::http::PostJsonRequest;
use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use rho_harness_core::text::truncate_chars;

const EXA_API_URL: &str = "https://api.exa.ai/search";
const EXA_MCP_URL: &str = "https://mcp.exa.ai/mcp?tools=web_search_exa";
const EXA_MCP_ADVANCED_URL: &str = "https://mcp.exa.ai/mcp?tools=web_search_advanced_exa";
pub(crate) const EXA_AUTH_PROVIDER: &str = "exa-search";
const EXA_MCP_SOURCE: &str = "rho";
const EXA_MCP_REQUEST_ID: &str = "1";
const MAX_EXA_SNIPPET_CHARS: usize = 500;
const MAX_EXA_RESPONSE_BYTES: usize = 2_000_000;
const MAX_ERROR_BODY_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
struct ExaApiResponse {
    results: Option<Vec<ExaRawResult>>,
}

#[derive(Debug, Deserialize)]
struct ExaRawResult {
    title: Option<String>,
    url: Option<String>,
    summary: Option<String>,
    text: Option<String>,
    highlights: Option<Vec<String>>,
}

pub(crate) async fn resolve_exa_api_key(auth_file: Option<&Path>) -> Option<String> {
    resolve_exa_api_key_with_env(auth_file, std::env::var("EXA_API_KEY").ok()).await
}

async fn resolve_exa_api_key_with_env(auth_file: Option<&Path>, env_fallback: Option<String>) -> Option<String> {
    let from_file = match auth_file {
        Some(path) => exa_key_from_auth_file(path).await,
        None => None,
    };
    from_file.or_else(|| env_fallback.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()))
}

async fn exa_key_from_auth_file(path: &Path) -> Option<String> {
    let mut store = AuthStore::load_async(path)
        .await
        .inspect_err(|error| tracing::warn!("reading {} for the Exa API key failed: {error}", path.display()))
        .ok()?;
    let key = store
        .get_key(EXA_AUTH_PROVIDER)
        .await
        .inspect_err(|error| tracing::warn!("resolving the Exa API key failed: {error}"))
        .ok()??;
    Some(key.trim().to_string()).filter(|k| !k.is_empty())
}

pub(crate) fn recency_to_start_published_date(recency: WebSearchRecency, now: DateTime<Utc>) -> String {
    let days = match recency {
        WebSearchRecency::Day => 1,
        WebSearchRecency::Week => 7,
        WebSearchRecency::Month => 30,
        WebSearchRecency::Year => 365,
    };
    (now - Duration::days(days)).to_rfc3339_opts(SecondsFormat::Secs, true)
}

pub(crate) fn build_exa_api_body(req: &EngineRequest<'_>, now: DateTime<Utc>) -> Value {
    let mut body = Map::new();
    body.insert("query".into(), json!(req.query));
    body.insert("numResults".into(), json!(req.limit.clamp(1, 20)));
    body.insert("type".into(), json!("auto"));
    body.insert("contents".into(), json!({ "summary": { "query": req.query } }));
    insert_native_filters(&mut body, req, now);
    Value::Object(body)
}

fn insert_native_filters(arguments: &mut Map<String, Value>, req: &EngineRequest<'_>, now: DateTime<Utc>) {
    if !req.allowed_domains.is_empty() {
        arguments.insert("includeDomains".into(), json!(req.allowed_domains));
    }
    if !req.blocked_domains.is_empty() {
        arguments.insert("excludeDomains".into(), json!(req.blocked_domains));
    }
    if let Some(recency) = req.recency {
        arguments.insert(
            "startPublishedDate".into(),
            json!(recency_to_start_published_date(recency, now)),
        );
    }
}

fn has_native_filters(req: &EngineRequest<'_>) -> bool {
    !req.allowed_domains.is_empty() || !req.blocked_domains.is_empty() || req.recency.is_some()
}

pub(crate) fn build_exa_mcp_body(req: &EngineRequest<'_>, request_id: &str, now: DateTime<Utc>) -> Value {
    let mut arguments = Map::new();
    arguments.insert("query".into(), json!(req.query));
    arguments.insert("numResults".into(), json!(req.limit.clamp(1, 20)));

    let tool_name = if has_native_filters(req) {
        insert_native_filters(&mut arguments, req, now);
        "web_search_advanced_exa"
    } else {
        "web_search_exa"
    };

    json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": Value::Object(arguments),
        }
    })
}

pub(crate) async fn search_exa(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    if let Some(api_key) = resolve_exa_api_key(req.auth_file).await {
        search_exa_api(req, &api_key, EXA_API_URL).await
    } else {
        let endpoint = if has_native_filters(req) {
            EXA_MCP_ADVANCED_URL
        } else {
            EXA_MCP_URL
        };
        search_exa_mcp(req, endpoint).await
    }
}

async fn search_exa_api(req: &EngineRequest<'_>, api_key: &str, endpoint: &str) -> Result<Vec<SearchResult>, AppError> {
    let body = build_exa_api_body(req, Utc::now());
    let resp = req
        .http
        .post_json(PostJsonRequest {
            url: endpoint,
            body: &body,
            headers: &[("x-api-key", api_key)],
            timeout_sec: req.timeout_sec,
            max_bytes: MAX_EXA_RESPONSE_BYTES,
        })
        .await?;

    if !resp.status.is_success() {
        let detail = truncate_text(&resp.body, MAX_ERROR_BODY_CHARS);
        return Err(AppError::Tool(format!(
            "Exa API error ({}): {detail}",
            resp.status.as_u16()
        )));
    }

    let parsed: ExaApiResponse = serde_json::from_str(&resp.body)
        .map_err(|e| AppError::Tool(format!("Failed to parse Exa API response: {e}")))?;
    Ok(convert_raw_results(parsed.results.unwrap_or_default()))
}

async fn search_exa_mcp(req: &EngineRequest<'_>, endpoint: &str) -> Result<Vec<SearchResult>, AppError> {
    let body = build_exa_mcp_body(req, EXA_MCP_REQUEST_ID, Utc::now());
    let resp = req
        .http
        .post_json_until(
            PostJsonRequest {
                url: endpoint,
                body: &body,
                headers: &[
                    ("Accept", "application/json, text/event-stream"),
                    ("x-exa-source", EXA_MCP_SOURCE),
                ],
                timeout_sec: req.timeout_sec,
                max_bytes: MAX_EXA_RESPONSE_BYTES,
            },
            |content_type, buf| {
                let text = String::from_utf8_lossy(buf);
                is_sse_payload(content_type, &text) && parse_sse_jsonrpc(&text, EXA_MCP_REQUEST_ID).is_ok()
            },
        )
        .await?;

    if resp.status == StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::Tool(
            "Exa MCP rate limit reached (429). Configure EXA_API_KEY or run `/login exa-search` for higher limits."
                .to_string(),
        ));
    }
    if !resp.status.is_success() {
        let detail = truncate_text(&resp.body, MAX_ERROR_BODY_CHARS);
        return Err(AppError::Tool(format!(
            "Exa MCP request failed ({}): {detail}",
            resp.status.as_u16()
        )));
    }

    parse_mcp_response(&resp.content_type, &resp.body, EXA_MCP_REQUEST_ID)
}

fn truncate_text(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    let (_, cut) = truncate_chars(trimmed, max_chars);
    if !cut {
        return trimmed.to_string();
    }
    let (prefix, _) = truncate_chars(trimmed, max_chars.saturating_sub(3));
    format!("{}...", prefix.trim_end())
}

fn pick_snippet(raw: &ExaRawResult) -> String {
    let candidate = raw
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| raw.text.as_deref().map(str::trim).filter(|s| !s.is_empty()));

    if let Some(text) = candidate {
        return truncate_text(text, MAX_EXA_SNIPPET_CHARS);
    }

    if let Some(highlights) = &raw.highlights {
        let joined: Vec<&str> = highlights
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !joined.is_empty() {
            return truncate_text(&joined.join(" "), MAX_EXA_SNIPPET_CHARS);
        }
    }

    String::new()
}

fn convert_raw_results(raw_results: Vec<ExaRawResult>) -> Vec<SearchResult> {
    raw_results
        .into_iter()
        .filter_map(|item| {
            let url = item
                .url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())?
                .to_string();
            let title = item
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .unwrap_or(&url)
                .to_string();
            let snippet = pick_snippet(&item);
            Some(SearchResult::new(title, snippet, url))
        })
        .collect()
}

fn parse_mcp_response(content_type: &str, body: &str, request_id: &str) -> Result<Vec<SearchResult>, AppError> {
    let envelope = extract_jsonrpc_message(content_type, body, request_id)?;
    if let Some(err) = envelope.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown MCP error");
        return Err(AppError::Tool(format!("Exa MCP error: {msg}")));
    }

    let result = envelope
        .get("result")
        .ok_or_else(|| AppError::Tool("Exa MCP response missing result field".to_string()))?;

    extract_mcp_results(result)
}

fn is_sse_payload(content_type: &str, body: &str) -> bool {
    if content_type.to_ascii_lowercase().contains("text/event-stream") {
        return true;
    }
    let trimmed = body.trim_start();
    trimmed.starts_with("event:") || trimmed.starts_with("data:")
}

fn is_matching_jsonrpc_response(msg: &Value, request_id: &str) -> bool {
    matches_request_id(msg, request_id) && (msg.get("result").is_some() || msg.get("error").is_some())
}

fn extract_jsonrpc_message(content_type: &str, body: &str, request_id: &str) -> Result<Value, AppError> {
    if is_sse_payload(content_type, body) {
        parse_sse_jsonrpc(body, request_id)
    } else {
        let parsed: Value = serde_json::from_str(body)
            .map_err(|e| AppError::Tool(format!("Failed to parse Exa MCP JSON response: {e}")))?;
        if !is_matching_jsonrpc_response(&parsed, request_id) {
            return Err(AppError::Tool(
                "Exa MCP JSON response did not match request ID".to_string(),
            ));
        }
        Ok(parsed)
    }
}

fn extract_sse_block_data(block: &str) -> Option<String> {
    let data_lines: Vec<&str> = block
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("data:")?;
            Some(rest.strip_prefix(' ').unwrap_or(rest))
        })
        .collect();
    if data_lines.is_empty() {
        return None;
    }
    let joined = data_lines.join("\n");
    (!joined.trim().is_empty()).then_some(joined)
}

fn matches_request_id(msg: &Value, request_id: &str) -> bool {
    match msg.get("id") {
        Some(Value::String(s)) => s == request_id,
        Some(Value::Number(n)) => n.to_string() == request_id,
        _ => false,
    }
}

fn parse_sse_jsonrpc(body: &str, request_id: &str) -> Result<Value, AppError> {
    let normalized = body.replace("\r\n", "\n");

    for block in normalized.split("\n\n") {
        let Some(data) = extract_sse_block_data(block) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if is_matching_jsonrpc_response(&parsed, request_id) {
            return Ok(parsed);
        }
    }

    Err(AppError::Tool(
        "Exa MCP SSE response contained no matching JSON-RPC response".to_string(),
    ))
}

fn extract_mcp_error_text(result: &Value) -> String {
    if let Some(items) = result.get("content").and_then(Value::as_array) {
        for item in items {
            if let Some(text) = item.get("text").and_then(Value::as_str).map(str::trim)
                && !text.is_empty()
            {
                return format!("Exa MCP error: {}", truncate_text(text, MAX_ERROR_BODY_CHARS));
            }
        }
    }
    "Exa MCP returned an error".to_string()
}

fn extract_mcp_results(result: &Value) -> Result<Vec<SearchResult>, AppError> {
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(AppError::Tool(extract_mcp_error_text(result)));
    }

    if let Some(results) = extract_structured_results(result) {
        return Ok(results);
    }

    if let Some(results) = parse_mcp_text_blocks(result) {
        return Ok(results);
    }

    Err(AppError::Tool(
        "Exa MCP search returned unexpected response shape".to_string(),
    ))
}

fn try_parse_results_array(value: &Value) -> Option<Vec<SearchResult>> {
    let arr = value.get("results")?.as_array()?;
    let raw: Vec<ExaRawResult> = serde_json::from_value(Value::Array(arr.clone())).ok()?;
    Some(convert_raw_results(raw))
}

fn extract_structured_results(result: &Value) -> Option<Vec<SearchResult>> {
    for key in ["structuredContent", "data", "result"] {
        if let Some(candidate) = result.get(key)
            && let Some(parsed) = try_parse_results_array(candidate)
        {
            return Some(parsed);
        }
    }
    if let Some(parsed) = try_parse_results_array(result) {
        return Some(parsed);
    }

    let items = result.get("content")?.as_array()?;
    for item in items {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            continue;
        };
        if let Ok(json_val) = serde_json::from_str::<Value>(text)
            && let Some(parsed) = try_parse_results_array(&json_val)
        {
            return Some(parsed);
        }
    }
    None
}

fn collect_content_text_blocks(result: &Value) -> Vec<String> {
    let Some(items) = result.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .map(|s| s.replace("\r\n", "\n").replace('\r', "\n").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_section_boundary(lines: &[&str], idx: usize) -> bool {
    if !lines[idx].starts_with("Title:") {
        return false;
    }
    let next = lines.get(idx + 1).copied().unwrap_or("");
    next.starts_with("URL:")
        || next.starts_with("Author:")
        || next.starts_with("Published Date:")
        || next.starts_with("Highlights:")
        || next.starts_with("Text:")
}

fn split_text_sections(combined: &str) -> Vec<String> {
    let lines: Vec<&str> = combined.lines().collect();
    let mut sections: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for (idx, &line) in lines.iter().enumerate() {
        if is_section_boundary(&lines, idx) && !current.is_empty() {
            sections.push(current.join("\n"));
            current.clear();
        }
        current.push(line);
    }
    if !current.is_empty() {
        sections.push(current.join("\n"));
    }

    sections
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| s.starts_with("Title:"))
        .collect()
}

fn parse_text_section(section: &str) -> Option<ExaRawResult> {
    let mut title: Option<String> = None;
    let mut url: Option<String> = None;
    let mut text_lines: Vec<&str> = Vec::new();
    let mut in_text = false;

    for line in section.lines() {
        if in_text {
            text_lines.push(line);
            continue;
        }
        if let Some(rest) = line.strip_prefix("Title:") {
            title = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("URL:") {
            url = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("Highlights:").or_else(|| line.strip_prefix("Text:")) {
            in_text = true;
            if !rest.trim().is_empty() {
                text_lines.push(rest.trim());
            }
        }
    }

    let text = text_lines.join("\n").trim().to_string();
    let text = (!text.is_empty()).then_some(text);
    if title.is_none() && url.is_none() && text.is_none() {
        return None;
    }

    Some(ExaRawResult {
        title,
        url,
        summary: None,
        text,
        highlights: None,
    })
}

fn parse_mcp_text_blocks(result: &Value) -> Option<Vec<SearchResult>> {
    let blocks = collect_content_text_blocks(result);
    if blocks.is_empty() {
        return None;
    }

    let combined = blocks.join("\n\n");
    let raw: Vec<ExaRawResult> = split_text_sections(&combined)
        .iter()
        .filter_map(|section| parse_text_section(section))
        .collect();

    if raw.is_empty() {
        if combined.to_ascii_lowercase().contains("no search results found") {
            return Some(Vec::new());
        }
        return None;
    }
    Some(convert_raw_results(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::web::http::HttpClient;
    use crate::tools::web::test_server::{CannedResponse, serve_once};
    use chrono::TimeZone;

    async fn spawn_mock_http_server(
        status: &'static str,
        content_type: &'static str,
        response_body: String,
    ) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let (base, captured) = serve_once(CannedResponse::new(status, content_type, response_body)).await;
        (format!("{base}search"), captured)
    }

    fn test_request<'a>(
        http: &'a HttpClient,
        query: &'a str,
        limit: usize,
        recency: Option<WebSearchRecency>,
        allowed: &'a [String],
        blocked: &'a [String],
    ) -> EngineRequest<'a> {
        EngineRequest {
            http,
            timeout_sec: 5,
            region: "us-en",
            query,
            limit,
            recency,
            allowed_domains: allowed,
            blocked_domains: blocked,
            auth_file: None,
        }
    }

    #[test]
    fn recency_maps_to_iso8601_start_published_date() {
        let now = Utc.with_ymd_and_hms(2026, 11, 24, 15, 30, 0).unwrap();
        assert_eq!(
            recency_to_start_published_date(WebSearchRecency::Day, now),
            "2026-11-23T15:30:00Z"
        );
        assert_eq!(
            recency_to_start_published_date(WebSearchRecency::Week, now),
            "2026-11-17T15:30:00Z"
        );
        assert_eq!(
            recency_to_start_published_date(WebSearchRecency::Month, now),
            "2026-10-25T15:30:00Z"
        );
        assert_eq!(
            recency_to_start_published_date(WebSearchRecency::Year, now),
            "2025-11-24T15:30:00Z"
        );
    }

    #[test]
    fn build_exa_api_body_maps_domains_and_recency() {
        let http = HttpClient::new(true).unwrap();
        let allowed = vec!["rust-lang.org".to_string(), "docs.rs".to_string()];
        let blocked = vec!["spam.example".to_string()];
        let req = test_request(
            &http,
            "async rust traits",
            8,
            Some(WebSearchRecency::Week),
            &allowed,
            &blocked,
        );
        let now = Utc.with_ymd_and_hms(2026, 11, 24, 12, 0, 0).unwrap();

        let body = build_exa_api_body(&req, now);
        assert_eq!(body["query"], "async rust traits");
        assert_eq!(body["numResults"], 8);
        assert_eq!(body["type"], "auto");
        assert_eq!(body["contents"]["summary"]["query"], "async rust traits");
        assert_eq!(body["includeDomains"], json!(["rust-lang.org", "docs.rs"]));
        assert_eq!(body["excludeDomains"], json!(["spam.example"]));
        assert_eq!(body["startPublishedDate"], "2026-11-17T12:00:00Z");
    }

    #[tokio::test]
    async fn direct_api_sends_key_header_and_parses_results() {
        let long_summary = "a".repeat(620);
        let response_body = json!({
            "results": [
                {
                    "title": "Rust Book",
                    "url": "https://doc.rust-lang.org/book/",
                    "summary": long_summary
                },
                {
                    "title": "Missing URL",
                    "summary": "should be dropped"
                },
                {
                    "url": "https://docs.rs/tokio",
                    "highlights": ["Fast async runtime", "for Rust"]
                }
            ]
        })
        .to_string();

        let (endpoint, captured_rx) = spawn_mock_http_server("200 OK", "application/json", response_body).await;
        let http = HttpClient::new(true).unwrap();
        let allowed = vec!["rust-lang.org".to_string()];
        let blocked = vec!["bad.com".to_string()];
        let req = test_request(&http, "rust async", 5, Some(WebSearchRecency::Day), &allowed, &blocked);

        let results = search_exa_api(&req, "exa-test-secret", &endpoint).await.unwrap();
        let raw_req = captured_rx.await.unwrap();
        let lower_headers = raw_req.to_ascii_lowercase();
        assert!(lower_headers.contains("x-api-key: exa-test-secret"));
        assert!(lower_headers.contains("content-type: application/json"));
        assert!(raw_req.contains("\"includeDomains\":[\"rust-lang.org\"]"));
        assert!(raw_req.contains("\"excludeDomains\":[\"bad.com\"]"));
        assert!(raw_req.contains("\"startPublishedDate\":"));

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Book");
        assert_eq!(results[0].url, "https://doc.rust-lang.org/book/");
        assert!(results[0].abstract_text.chars().count() <= MAX_EXA_SNIPPET_CHARS);
        assert!(results[0].abstract_text.ends_with("..."));

        assert_eq!(results[1].title, "https://docs.rs/tokio");
        assert_eq!(results[1].abstract_text, "Fast async runtime for Rust");
    }

    #[tokio::test]
    async fn mcp_fallback_parses_json_response() {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "content": [{
                    "type": "text",
                    "text": json!({
                        "results": [{
                            "title": "Exa JSON Result",
                            "url": "https://example.com/json",
                            "text": "Structured JSON inside MCP content."
                        }]
                    }).to_string()
                }]
            }
        })
        .to_string();

        let (endpoint, captured_rx) = spawn_mock_http_server("200 OK", "application/json", payload).await;
        let http = HttpClient::new(true).unwrap();
        let req = test_request(&http, "exa mcp json", 5, None, &[], &[]);

        let results = search_exa_mcp(&req, &endpoint).await.unwrap();
        let raw_req = captured_rx.await.unwrap();
        assert!(
            raw_req
                .to_ascii_lowercase()
                .contains("accept: application/json, text/event-stream")
        );
        assert!(raw_req.contains("\"method\":\"tools/call\""));
        assert!(raw_req.contains("\"name\":\"web_search_exa\""));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Exa JSON Result");
        assert_eq!(results[0].url, "https://example.com/json");
        assert_eq!(results[0].abstract_text, "Structured JSON inside MCP content.");
    }

    #[tokio::test]
    async fn mcp_fallback_parses_sse_and_plain_text_blocks() {
        let text_blocks = concat!(
            "Title: First Article\n",
            "URL: https://example.com/first\n",
            "Author: Alice\n",
            "Text: Summary of first article.\n",
            "Second line of snippet.\n\n",
            "Title: Second Article\n",
            "URL: https://example.com/second\n",
            "Text: Summary of second article."
        );
        let msg = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "content": [{ "type": "text", "text": text_blocks }]
            }
        })
        .to_string();
        let sse_body = format!("event: message\ndata: {msg}\n\n");

        let (endpoint, _) = spawn_mock_http_server("200 OK", "text/event-stream", sse_body).await;
        let http = HttpClient::new(true).unwrap();
        let req = test_request(&http, "sse test", 5, None, &[], &[]);

        let results = search_exa_mcp(&req, &endpoint).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "First Article");
        assert_eq!(results[0].url, "https://example.com/first");
        assert!(results[0].abstract_text.contains("Summary of first article."));
        assert!(results[0].abstract_text.contains("Second line of snippet."));
        assert_eq!(results[1].title, "Second Article");
        assert_eq!(results[1].url, "https://example.com/second");
    }

    #[tokio::test]
    async fn mcp_fallback_429_returns_actionable_error() {
        let (endpoint, _) = spawn_mock_http_server(
            "429 Too Many Requests",
            "application/json",
            r#"{"error":"rate limit"}"#.to_string(),
        )
        .await;
        let http = HttpClient::new(true).unwrap();
        let req = test_request(&http, "rate limited", 5, None, &[], &[]);

        let err = search_exa_mcp(&req, &endpoint).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("429"), "{msg}");
        assert!(msg.contains("EXA_API_KEY"), "{msg}");
        assert!(msg.contains("/login exa-search"), "{msg}");
    }

    #[tokio::test]
    async fn mcp_fallback_parses_highlights_and_no_results_and_validates_id() {
        let highlights_block = concat!(
            "Title: Highlighted Article\n",
            "URL: https://example.com/hl\n",
            "Highlights: Key excerpt from highlights.\n",
            "More highlight context."
        );
        let msg = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "content": [{ "type": "text", "text": highlights_block }]
            }
        })
        .to_string();
        let results =
            parse_mcp_response("text/event-stream", &format!("event: message\ndata: {msg}\n\n"), "1").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Highlighted Article");
        assert!(results[0].abstract_text.contains("Key excerpt from highlights."));

        let empty_msg = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "result": {
                "content": [{ "type": "text", "text": "No search results found. Please try a different query." }]
            }
        })
        .to_string();
        let empty = parse_mcp_response("application/json", &empty_msg, "1").unwrap();
        assert!(empty.is_empty());

        let wrong_id = json!({
            "jsonrpc": "2.0",
            "id": "999",
            "result": {
                "content": [{ "type": "text", "text": highlights_block }]
            }
        })
        .to_string();
        assert!(parse_mcp_response("application/json", &wrong_id, "1").is_err());
        assert!(parse_mcp_response("text/event-stream", &format!("data: {wrong_id}\n\n"), "1").is_err());
    }

    #[test]
    fn build_exa_mcp_body_uses_advanced_tool_for_native_filters() {
        let http = HttpClient::new(true).unwrap();
        let allowed = vec!["rust-lang.org".to_string()];
        let blocked = vec!["spam.example".to_string()];
        let req = test_request(&http, "async", 5, Some(WebSearchRecency::Day), &allowed, &blocked);
        let now = Utc.with_ymd_and_hms(2026, 11, 24, 12, 0, 0).unwrap();
        let body = build_exa_mcp_body(&req, "1", now);
        assert_eq!(body["params"]["name"], "web_search_advanced_exa");
        assert_eq!(body["params"]["arguments"]["includeDomains"], json!(["rust-lang.org"]));
        assert_eq!(body["params"]["arguments"]["excludeDomains"], json!(["spam.example"]));
        assert_eq!(
            body["params"]["arguments"]["startPublishedDate"],
            "2026-11-23T12:00:00Z"
        );
    }

    #[tokio::test]
    async fn resolve_exa_api_key_prefers_auth_store_and_falls_back_to_env() {
        let dir = rho_test_support::TestDir::new();
        let auth_path = dir.join("auth.json");
        let empty_resolved = resolve_exa_api_key_with_env(Some(&auth_path), Some("exa-env-key-456".to_string())).await;
        assert_eq!(empty_resolved.as_deref(), Some("exa-env-key-456"));

        let mut store = AuthStore::load(&auth_path).unwrap();
        store.set_key(EXA_AUTH_PROVIDER, "exa-stored-key-123").unwrap();
        let stored_resolved = resolve_exa_api_key_with_env(Some(&auth_path), Some("exa-env-key-456".to_string())).await;
        assert_eq!(stored_resolved.as_deref(), Some("exa-stored-key-123"));
    }

    #[tokio::test]
    async fn an_unreadable_auth_file_falls_back_to_env() {
        let dir = rho_test_support::TestDir::new();
        let auth_path = dir.join("auth.json");
        std::fs::write(&auth_path, "not json").unwrap();
        let resolved = resolve_exa_api_key_with_env(Some(&auth_path), Some("exa-env-key-456".to_string())).await;
        assert_eq!(resolved.as_deref(), Some("exa-env-key-456"));
    }
}
