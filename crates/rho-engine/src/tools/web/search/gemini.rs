use crate::tools::web::search::engine::EngineRequest;
use crate::tools::web::search::result::SearchResult;
use rho_harness_core::error::AppError;
use serde::Deserialize;

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent";

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    pub content: Option<GeminiContent>,
    #[serde(rename = "groundingMetadata")]
    pub grounding_metadata: Option<GeminiGroundingMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    pub parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiGroundingMetadata {
    #[serde(rename = "groundingChunks")]
    pub grounding_chunks: Option<Vec<GeminiGroundingChunk>>,
}

#[derive(Debug, Deserialize)]
struct GeminiGroundingChunk {
    pub web: Option<GeminiWebChunk>,
}

#[derive(Debug, Deserialize)]
struct GeminiWebChunk {
    pub uri: Option<String>,
    pub title: Option<String>,
}

pub fn build_gemini_grounding_payload(query: &str) -> serde_json::Value {
    serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": format!("Search the web and provide summary information for: {query}")
                    }
                ]
            }
        ],
        "tools": [
            {
                "googleSearch": {}
            }
        ]
    })
}

pub fn parse_gemini_grounding_json(json_str: &str) -> Vec<SearchResult> {
    let parsed: GeminiResponse = match serde_json::from_str(json_str) {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let Some(candidates) = parsed.candidates else {
        return Vec::new();
    };

    let Some(first_candidate) = candidates.first() else {
        return Vec::new();
    };

    let answer_text = extract_candidate_text(first_candidate);
    let mut results = Vec::new();
    if let Some(meta) = &first_candidate.grounding_metadata
        && let Some(chunks) = &meta.grounding_chunks
    {
        for chunk in chunks {
            let Some(web) = &chunk.web else {
                continue;
            };
            let Some(uri) = &web.uri else {
                continue;
            };
            let title = web.title.as_deref().unwrap_or_default();
            results.push(SearchResult::new(title, &answer_text, uri).with_source("Gemini"));
        }
    }

    results
}

fn extract_candidate_text(candidate: &GeminiCandidate) -> String {
    let mut text_parts = Vec::new();
    if let Some(content) = &candidate.content
        && let Some(parts) = &content.parts
    {
        for part in parts {
            if let Some(text) = &part.text
                && !text.trim().is_empty()
            {
                text_parts.push(text.trim());
            }
        }
    }
    text_parts.join("\n")
}

pub async fn search_gemini(req: &EngineRequest<'_>) -> Result<Vec<SearchResult>, AppError> {
    let api_key = match std::env::var("GEMINI_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k.trim().to_string(),
        _ => {
            return Err(AppError::Auth(
                "Missing GEMINI_API_KEY environment variable for Gemini grounded search".to_string(),
            ));
        }
    };

    let payload = build_gemini_grounding_payload(req.query);

    let resp = req
        .http
        .client
        .post(format!("{GEMINI_API_URL}?key={api_key}"))
        .header("Content-Type", "application/json")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(req.timeout_sec))
        .send()
        .await
        .map_err(|e| AppError::Tool(format!("Gemini grounded search request failed: {e}")))?;

    if resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        Ok(parse_gemini_grounding_json(&body))
    } else {
        let status = resp.status();
        Err(AppError::Tool(format!("Gemini grounded search returned HTTP {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_gemini_grounding_payload() {
        let payload = build_gemini_grounding_payload("latest Rust features");
        let contents = payload.get("contents").and_then(|c| c.as_array()).unwrap();
        assert_eq!(contents.len(), 1);
        let tools = payload.get("tools").and_then(|t| t.as_array()).unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].get("googleSearch").is_some());
    }

    #[test]
    fn test_parse_gemini_grounding_json_with_chunks() {
        let json = r#"{
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "Rust 2024 edition stabilizes new features."
                            }
                        ]
                    },
                    "groundingMetadata": {
                        "groundingChunks": [
                            {
                                "web": {
                                    "uri": "https://blog.rust-lang.org/edition-2024",
                                    "title": "Rust 2024 Edition"
                                }
                            },
                            {
                                "web": {
                                    "uri": "https://doc.rust-lang.org/edition-guide",
                                    "title": "Rust Edition Guide"
                                }
                            }
                        ]
                    }
                }
            ]
        }"#;

        let results = parse_gemini_grounding_json(json);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust 2024 Edition");
        assert_eq!(results[0].url, "https://blog.rust-lang.org/edition-2024");
        assert_eq!(results[0].abstract_text, "Rust 2024 edition stabilizes new features.");
        assert_eq!(results[0].source.as_deref(), Some("Gemini"));
        assert_eq!(results[1].title, "Rust Edition Guide");
        assert_eq!(results[1].url, "https://doc.rust-lang.org/edition-guide");
    }

    #[test]
    fn test_parse_gemini_grounding_json_without_grounding_metadata() {
        let json = r#"{
            "candidates": [
                {
                    "content": {
                        "parts": [
                            {
                                "text": "Direct answer without search."
                            }
                        ]
                    }
                }
            ]
        }"#;

        let results = parse_gemini_grounding_json(json);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_gemini_grounding_json_empty_and_malformed() {
        assert!(parse_gemini_grounding_json("").is_empty());
        assert!(parse_gemini_grounding_json("{}").is_empty());
        assert!(parse_gemini_grounding_json(r#"{"candidates": []}"#).is_empty());
    }
}
