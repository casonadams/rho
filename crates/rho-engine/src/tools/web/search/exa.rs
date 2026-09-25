use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::result::SearchResult;
use chrono::{Duration, Utc};
use rho_harness_core::args::WebSearchRecency;
use rho_harness_core::error::AppError;
use serde::Deserialize;

const EXA_SEARCH_URL: &str = "https://api.exa.ai/search";

#[derive(Debug, Deserialize)]
struct ExaSearchResponse {
    pub results: Option<Vec<ExaResultItem>>,
}

#[derive(Debug, Deserialize)]
struct ExaResultItem {
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub highlights: Option<Vec<String>>,
    pub text: Option<String>,
}

pub fn recency_to_iso_published_date(recency: WebSearchRecency) -> String {
    let now = Utc::now();
    let days = match recency {
        WebSearchRecency::Day => 1,
        WebSearchRecency::Week => 7,
        WebSearchRecency::Month => 30,
        WebSearchRecency::Year => 365,
    };
    (now - Duration::days(days)).to_rfc3339()
}

pub fn build_exa_payload(
    query: &str,
    recency: Option<WebSearchRecency>,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "query": query,
        "type": "auto",
        "numResults": 10,
        "contents": {
            "highlights": true,
            "summary": true
        }
    });

    if let Some(r) = recency {
        payload["startPublishedDate"] = serde_json::Value::String(recency_to_iso_published_date(r));
    }

    if !allowed_domains.is_empty() {
        payload["includeDomains"] = serde_json::json!(allowed_domains);
    }
    if !blocked_domains.is_empty() {
        payload["excludeDomains"] = serde_json::json!(blocked_domains);
    }

    payload
}

pub fn parse_exa_json(json_str: &str) -> Vec<SearchResult> {
    let parsed: ExaSearchResponse = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let Some(items) = parsed.results else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for item in &items {
        let Some(url) = &item.url else {
            continue;
        };
        let snippet = extract_snippet(item);
        let title = item.title.clone().unwrap_or_default();
        results.push(SearchResult::new(title, snippet, url).with_source("Exa"));
    }
    results
}

fn extract_snippet(item: &ExaResultItem) -> String {
    if let Some(summary) = &item.summary
        && !summary.trim().is_empty()
    {
        return summary.trim().to_string();
    }
    if let Some(hl) = &item.highlights
        && let Some(first) = hl.first()
        && !first.trim().is_empty()
    {
        return first.trim().to_string();
    }
    item.text.as_deref().unwrap_or_default().trim().to_string()
}
pub async fn search_exa(
    req: &EngineRequest<'_>,
    allowed_domains: &[String],
    blocked_domains: &[String],
) -> Result<Vec<SearchResult>, AppError> {
    let api_key = match std::env::var("EXA_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => {
            return Err(AppError::Auth(
                "Missing EXA_API_KEY environment variable for Exa search".to_string(),
            ));
        }
    };

    let payload = build_exa_payload(req.query, req.recency, allowed_domains, blocked_domains);

    let resp = req
        .http
        .client
        .post(EXA_SEARCH_URL)
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(req.timeout_sec))
        .send()
        .await
        .map_err(|e| AppError::Tool(format!("Exa search request failed: {e}")))?;

    if resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        Ok(parse_exa_json(&body))
    } else {
        let status = resp.status();
        Err(AppError::Tool(format!("Exa search returned HTTP {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recency_to_iso_published_date() {
        let day = recency_to_iso_published_date(WebSearchRecency::Day);
        assert!(day.contains('T'));
        let week = recency_to_iso_published_date(WebSearchRecency::Week);
        assert!(week.contains('T'));
    }

    #[test]
    fn test_build_exa_payload_with_domains_and_recency() {
        let allowed = vec!["crates.io".to_string(), "docs.rs".to_string()];
        let blocked = vec!["spam.com".to_string()];
        let payload = build_exa_payload("rust async runtime", Some(WebSearchRecency::Month), &allowed, &blocked);

        assert_eq!(payload["query"], "rust async runtime");
        assert_eq!(payload["type"], "auto");
        assert_eq!(payload["numResults"], 10);
        assert_eq!(payload["includeDomains"], serde_json::json!(["crates.io", "docs.rs"]));
        assert_eq!(payload["excludeDomains"], serde_json::json!(["spam.com"]));
        assert!(payload["startPublishedDate"].is_string());
        assert_eq!(payload["contents"]["highlights"], true);
        assert_eq!(payload["contents"]["summary"], true);
    }

    #[test]
    fn test_build_exa_payload_minimal() {
        let payload = build_exa_payload("test", None, &[], &[]);
        assert_eq!(payload["query"], "test");
        assert!(payload.get("includeDomains").is_none());
        assert!(payload.get("excludeDomains").is_none());
        assert!(payload.get("startPublishedDate").is_none());
    }

    #[test]
    fn test_parse_exa_json_summary_priority() {
        let json = r#"{
            "results": [
                {
                    "title": "Tokio",
                    "url": "https://tokio.rs",
                    "summary": "An asynchronous runtime for Rust",
                    "highlights": ["Tokio is an async runtime"],
                    "text": "Full text here"
                }
            ]
        }"#;

        let results = parse_exa_json(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tokio");
        assert_eq!(results[0].url, "https://tokio.rs");
        assert_eq!(results[0].abstract_text, "An asynchronous runtime for Rust");
        assert_eq!(results[0].source.as_deref(), Some("Exa"));
    }

    #[test]
    fn test_parse_exa_json_highlights_fallback() {
        let json = r#"{
            "results": [
                {
                    "title": "Tokio Docs",
                    "url": "https://docs.rs/tokio",
                    "highlights": ["Tokio documentation snippet"],
                    "text": "Full text here"
                }
            ]
        }"#;

        let results = parse_exa_json(json);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].abstract_text, "Tokio documentation snippet");
    }

    #[test]
    fn test_parse_exa_json_empty_and_malformed() {
        assert!(parse_exa_json("").is_empty());
        assert!(parse_exa_json("{}").is_empty());
        assert!(parse_exa_json(r#"{"results": []}"#).is_empty());
    }
}
