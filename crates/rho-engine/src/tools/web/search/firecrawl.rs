use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::error::AppError;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FirecrawlResponse {
    pub success: Option<bool>,
    pub data: Option<FirecrawlData>,
}

#[derive(Debug, Deserialize)]
struct FirecrawlData {
    pub web: Option<Vec<FirecrawlWebResult>>,
}

#[derive(Debug, Deserialize)]
struct FirecrawlWebResult {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
}

pub async fn search_firecrawl(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let payload = serde_json::json!({
        "query": req.query,
        "limit": 10,
        "sources": ["web"]
    });

    let resp = req
        .http
        .client
        .post("https://api.firecrawl.dev/v2/search")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(req.timeout_sec))
        .send()
        .await
        .map_err(|e| AppError::Tool(format!("Firecrawl request failed: {e}")))?;

    if resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        Ok(parse_firecrawl_json(&body))
    } else {
        Err(AppError::Tool("Firecrawl search error".to_string()))
    }
}

pub fn parse_firecrawl_json(json_str: &str) -> Vec<SearchResult> {
    let parsed: FirecrawlResponse = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    if parsed.success == Some(false) {
        return Vec::new();
    }

    let mut results = Vec::new();
    let Some(data) = parsed.data else {
        return results;
    };
    let Some(items) = data.web else {
        return results;
    };

    for item in items {
        if let Some(url) = item.url {
            let title = item.title.unwrap_or_default();
            let desc = item.description.unwrap_or_default();
            results.push(SearchResult::new(title, desc, url));
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_firecrawl_json() {
        let json = r#"{
            "success": true,
            "data": {
                "web": [
                    {
                        "title": "Crates.io",
                        "description": "The Rust package registry",
                        "url": "https://crates.io"
                    }
                ]
            }
        }"#;
        let res = parse_firecrawl_json(json);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "Crates.io");
        assert_eq!(res[0].url, "https://crates.io");
    }
}
