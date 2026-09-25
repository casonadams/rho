use crate::auth::store::AuthStore;
use crate::tools::web::http::HttpClient;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rho_harness_core::error::AppError;
use serde::Deserialize;
use std::path::Path;

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent";
pub const GEMINI_MAX_PAYLOAD_BYTES: usize = 20 * 1024 * 1024;
const LOW_CONFIDENCE_CHAR_THRESHOLD: usize = 50;
const LOW_CONFIDENCE_BYTE_THRESHOLD: usize = 10_240;

const IMAGE_PROMPT: &str = "Analyze this image and provide a comprehensive structured Markdown summary. Extract any text via OCR, transcribe any tables into GFM Markdown tables, and describe visual features, diagrams, or charts accurately.";
const SVG_PROMPT_PREFIX: &str = "Analyze this SVG diagram and provide a comprehensive structured Markdown summary. Describe the visual layout, nodes, connections, text labels, and data flows, and transcribe any data tables or charts in GFM Markdown table format.\n\n```xml\n";
const SVG_PROMPT_SUFFIX: &str = "\n```";
const PDF_PROMPT: &str = "Extract and reconstruct the full content of this document into clean, structured GFM Markdown. Transcribe all text, headings, lists, tables (in GFM table format), and visual diagrams. If sections or tables are scanned or poorly formatted, reconstruct them into clean, well-organized Markdown preserving logical structure and reading order.";

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    pub content: Option<GeminiContent>,
}

#[derive(Debug, Deserialize)]
struct GeminiContent {
    pub parts: Option<Vec<GeminiPart>>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart {
    pub text: Option<String>,
}

pub fn is_low_confidence_pdf(extracted_text: &str, byte_len: usize) -> bool {
    let trimmed = extracted_text.trim();
    trimmed.is_empty() || (trimmed.len() < LOW_CONFIDENCE_CHAR_THRESHOLD && byte_len >= LOW_CONFIDENCE_BYTE_THRESHOLD)
}

fn check_payload_size(len: usize) -> Result<(), AppError> {
    if len > GEMINI_MAX_PAYLOAD_BYTES {
        Err(AppError::Tool(format!(
            "Payload too large for Gemini multimodal analysis ({len} bytes; limit is 20MB)"
        )))
    } else {
        Ok(())
    }
}

pub fn build_image_payload(bytes: &[u8], mime_type: &str) -> Result<serde_json::Value, AppError> {
    check_payload_size(bytes.len())?;
    let b64 = STANDARD.encode(bytes);
    Ok(serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "inlineData": {
                            "mimeType": mime_type,
                            "data": b64
                        }
                    },
                    {
                        "text": IMAGE_PROMPT
                    }
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.2
        }
    }))
}

pub fn build_svg_payload(svg_text: &str) -> Result<serde_json::Value, AppError> {
    check_payload_size(svg_text.len())?;
    let prompt = format!("{SVG_PROMPT_PREFIX}{svg_text}{SVG_PROMPT_SUFFIX}");
    Ok(serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "text": prompt
                    }
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.2
        }
    }))
}

pub fn build_pdf_payload(bytes: &[u8]) -> Result<serde_json::Value, AppError> {
    check_payload_size(bytes.len())?;
    let b64 = STANDARD.encode(bytes);
    Ok(serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [
                    {
                        "inlineData": {
                            "mimeType": "application/pdf",
                            "data": b64
                        }
                    },
                    {
                        "text": PDF_PROMPT
                    }
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.2
        }
    }))
}

pub fn parse_gemini_multimodal_response(json_str: &str) -> Result<String, AppError> {
    let parsed: GeminiResponse =
        serde_json::from_str(json_str).map_err(|e| AppError::Tool(format!("Failed to parse Gemini response: {e}")))?;
    let candidate = parsed
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .ok_or_else(|| AppError::Tool("Gemini returned no candidates in multimodal analysis".to_string()))?;
    let mut texts = Vec::new();
    if let Some(content) = &candidate.content
        && let Some(parts) = &content.parts
    {
        for part in parts {
            if let Some(t) = &part.text
                && !t.trim().is_empty()
            {
                texts.push(t.trim());
            }
        }
    }
    if texts.is_empty() {
        return Err(AppError::Tool(
            "Gemini returned empty text in multimodal analysis".to_string(),
        ));
    }
    Ok(texts.join("\n\n"))
}

pub fn resolve_gemini_api_key(auth_file: Option<&Path>) -> Result<String, AppError> {
    for env_var in ["GEMINI_API_KEY", "GOOGLE_API_KEY"] {
        if let Ok(k) = std::env::var(env_var)
            && !k.trim().is_empty()
        {
            return Ok(k.trim().to_string());
        }
    }
    if let Some(path) = auth_file
        && let Ok(store) = AuthStore::load(path)
    {
        for provider in ["gemini", "google"] {
            if let Ok(Some(k)) = store.get_key_sync(provider)
                && !k.trim().is_empty()
            {
                return Ok(k.trim().to_string());
            }
        }
    }
    Err(AppError::Auth(
        "Missing GEMINI_API_KEY environment variable or stored credential for multimodal analysis. Run `rho login gemini` or set GEMINI_API_KEY.".to_string(),
    ))
}

pub async fn analyze_multimodal(
    http: &HttpClient,
    auth_file: Option<&Path>,
    payload: serde_json::Value,
    timeout_sec: u64,
) -> Result<String, AppError> {
    let api_key = resolve_gemini_api_key(auth_file)?;
    let url = format!("{GEMINI_API_URL}?key={api_key}");

    let resp = http
        .client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(timeout_sec))
        .send()
        .await
        .map_err(|e| AppError::Tool(format!("Gemini multimodal request failed: {e}")))?;

    let status = resp.status();
    if status.is_success() {
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Tool(format!("Failed to read Gemini response body: {e}")))?;
        parse_gemini_multimodal_response(&body)
    } else {
        let err_body = resp.text().await.unwrap_or_default();
        let safe_err = err_body.split(&api_key).collect::<Vec<_>>().join("[redacted]");
        Err(AppError::Tool(format!(
            "Gemini multimodal analysis returned HTTP {status}: {safe_err}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_low_confidence_pdf() {
        assert!(is_low_confidence_pdf("", 100));
        assert!(is_low_confidence_pdf("   \n\t  ", 20_000));
        assert!(is_low_confidence_pdf("short text", 15_000));
        assert!(!is_low_confidence_pdf("short text", 5_000));
        assert!(!is_low_confidence_pdf(
            "This is a rich digital PDF with substantial text content that should not trigger low confidence.",
            15_000
        ));
    }

    #[test]
    fn test_build_image_payload_structure() {
        let dummy = b"fake-png-bytes";
        let payload = build_image_payload(dummy, "image/png").unwrap();
        assert_eq!(payload["contents"][0]["role"], "user");
        let inline = &payload["contents"][0]["parts"][0]["inlineData"];
        assert_eq!(inline["mimeType"], "image/png");
        assert_eq!(inline["data"], STANDARD.encode(dummy));
        assert!(
            payload["contents"][0]["parts"][1]["text"]
                .as_str()
                .unwrap()
                .contains("Analyze this image")
        );
    }

    #[test]
    fn test_build_svg_payload_structure() {
        let svg = "<svg><circle cx='5' cy='5' r='5'/></svg>";
        let payload = build_svg_payload(svg).unwrap();
        let prompt = payload["contents"][0]["parts"][0]["text"].as_str().unwrap();
        assert!(prompt.contains(svg));
        assert!(prompt.contains("Analyze this SVG diagram"));
    }

    #[test]
    fn test_build_pdf_payload_structure() {
        let dummy = b"%PDF-1.4...";
        let payload = build_pdf_payload(dummy).unwrap();
        let inline = &payload["contents"][0]["parts"][0]["inlineData"];
        assert_eq!(inline["mimeType"], "application/pdf");
        assert_eq!(inline["data"], STANDARD.encode(dummy));
    }

    #[test]
    fn test_payload_exceeds_size_limit() {
        let oversized = vec![0u8; GEMINI_MAX_PAYLOAD_BYTES + 1];
        let err = build_image_payload(&oversized, "image/png").unwrap_err();
        assert!(err.to_string().contains("Payload too large"));
    }

    #[test]
    fn test_parse_gemini_multimodal_response_success() {
        let json = serde_json::json!({
            "candidates": [
                {
                    "content": {
                        "parts": [
                            { "text": "# Diagram Summary\n\nShows component flow." },
                            { "text": "| Step | Description |\n| --- | --- |\n| 1 | Init |" }
                        ]
                    }
                }
            ]
        })
        .to_string();

        let parsed = parse_gemini_multimodal_response(&json).unwrap();
        assert!(parsed.contains("# Diagram Summary"));
        assert!(parsed.contains("| Step | Description |"));
    }

    #[test]
    fn test_parse_gemini_multimodal_response_empty() {
        let json = serde_json::json!({
            "candidates": [
                {
                    "content": {
                        "parts": []
                    }
                }
            ]
        })
        .to_string();

        let err = parse_gemini_multimodal_response(&json).unwrap_err();
        assert!(err.to_string().contains("empty text"));
    }

    #[test]
    fn test_resolve_gemini_api_key_from_store() {
        let temp = tempfile::tempdir().unwrap();
        let auth_file = temp.path().join("auth.json");
        std::fs::write(&auth_file, r#"{"gemini": "test-gemini-stored-key"}"#).unwrap();

        let resolved = resolve_gemini_api_key(Some(&auth_file));
        if let Ok(env_key) = std::env::var("GEMINI_API_KEY")
            && !env_key.trim().is_empty()
        {
            assert_eq!(resolved.unwrap(), env_key.trim());
            return;
        }
        assert_eq!(resolved.unwrap(), "test-gemini-stored-key");
    }
}
