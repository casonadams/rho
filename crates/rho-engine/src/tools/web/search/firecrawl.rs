use crate::tools::web::search::result::SearchResult;
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
