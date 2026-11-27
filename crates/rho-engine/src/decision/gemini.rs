//! First-class Google Gemini Decisions API client (`POST /v1beta/decisions`).
//!
//! Targets `gemini-decision-1` (262k context, calibrated `noul`/`choice`/`score` answers,
//! multimodal text/image inputs, $0.10/1M input tokens, $0 output tokens).

use super::post_decision;
use super::types::{
    DecisionAnswer, DecisionInput, DecisionModel, DecisionPart, DecisionQuestion, DecisionRequest, DecisionResponse,
    DecisionUsage,
};
use crate::gemini::client::DEFAULT_BASE_URL;
use crate::http::header_value;
use crate::provider::error::{HttpFailure, error_message_or_excerpt};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
use rho_harness_core::error::AppError;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct GeminiDecisionClient {
    base_url: String,
    api_key: String,
    model: String,
}

impl std::fmt::Debug for GeminiDecisionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiDecisionClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl GeminiDecisionClient {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let mut base = base_url.into();
        while base.ends_with('/') {
            base.pop();
        }
        self.base_url = base;
        self
    }

    fn endpoint_url(&self) -> String {
        format!("{}/decisions", self.base_url)
    }

    fn headers(&self) -> Result<HeaderMap, HttpFailure> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-goog-api-key",
            header_value(self.api_key.trim(), "The Gemini API key")?,
        );
        Ok(headers)
    }
}

fn serialize_input(input: &DecisionInput) -> Value {
    match input {
        DecisionInput::Text(text) => json!(text),
        DecisionInput::Parts(parts) => Value::Array(
            parts
                .iter()
                .map(|part| match part {
                    DecisionPart::Text(text) => json!({ "type": "text", "text": text }),
                    DecisionPart::ImageBase64 { data, mime_type } => json!({
                        "type": "image",
                        "data": data,
                        "mime_type": mime_type,
                    }),
                })
                .collect(),
        ),
    }
}

fn fold_noul_instructions(instructions: &str, criteria_true: Option<&str>, criteria_false: Option<&str>) -> String {
    let mut out = instructions.to_string();
    if let Some(t) = criteria_true.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\nYes if: ");
        out.push_str(t);
    }
    if let Some(f) = criteria_false.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\nNo if: ");
        out.push_str(f);
    }
    out
}

fn serialize_question(question: &DecisionQuestion) -> Value {
    match question {
        DecisionQuestion::Noul {
            instructions,
            criteria_true,
            criteria_false,
        } => json!({
            "type": "noul",
            "instructions": fold_noul_instructions(instructions, criteria_true.as_deref(), criteria_false.as_deref()),
        }),
        DecisionQuestion::Choice { instructions, criteria } => json!({
            "type": "choice",
            "instructions": instructions,
            "criteria": criteria,
        }),
        DecisionQuestion::Score { instructions, criteria } => json!({
            "type": "score",
            "instructions": instructions,
            "criteria": criteria,
        }),
    }
}

pub fn build_request_body(model: &str, request: &DecisionRequest) -> Value {
    let clean_model = model.strip_prefix("models/").unwrap_or(model);
    let mut questions = Map::new();
    for (key, question) in &request.questions {
        questions.insert(key.clone(), serialize_question(question));
    }
    json!({
        "model": clean_model,
        "input": serialize_input(&request.input),
        "questions": Value::Object(questions),
    })
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    total_input_tokens: u64,
    #[serde(default)]
    total_output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

#[derive(Deserialize)]
struct WireResponse {
    #[serde(default)]
    answers: BTreeMap<String, DecisionAnswer>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

fn friendly_error(status: Option<u16>, body: &str) -> String {
    let msg = error_message_or_excerpt(body);
    match status {
        Some(401) => "Gemini Decisions API key rejected (401). Check GEMINI_API_KEY.".to_string(),
        Some(code) => format!("Gemini Decisions API error ({code}): {msg}"),
        None => format!("Gemini Decisions request failed: {msg}"),
    }
}

#[async_trait::async_trait]
impl DecisionModel for GeminiDecisionClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "gemini"
    }

    fn is_native_decision(&self) -> bool {
        true
    }

    async fn decide(&self, request: DecisionRequest) -> Result<DecisionResponse, AppError> {
        let body = build_request_body(&self.model, &request);
        let parsed: WireResponse = post_decision(
            &self.endpoint_url(),
            self.headers(),
            &body,
            "Gemini Decisions",
            friendly_error,
        )
        .await?;
        let usage = parsed.usage.map_or_else(DecisionUsage::default, |u| DecisionUsage {
            input_tokens: u.total_input_tokens,
            output_tokens: u.total_output_tokens,
            total_tokens: u.total_tokens,
        });
        Ok(DecisionResponse {
            answers: parsed.answers,
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn debug_redacts_api_key() {
        let client = GeminiDecisionClient::new("AIza-secret", "gemini-decision-1");
        let debug = format!("{client:?}");
        assert!(!debug.contains("AIza-secret"), "{debug}");
        assert!(debug.contains("gemini-decision-1"));
    }

    #[tokio::test]
    async fn decides_over_loopback_with_native_v1beta_decisions_wire_shape() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let payload = r#"{
                "answers": {
                    "is_safe": { "type": "noul", "noul": 0.96, "confidence": 0.96 },
                    "risk": { "type": "choice", "choice": "safe_local", "probabilities": { "safe_local": 0.99, "destructive": 0.01 }, "confidence": 0.99 }
                },
                "usage": { "total_input_tokens": 120, "total_output_tokens": 0, "total_tokens": 120 }
            }"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            req
        });

        let client =
            GeminiDecisionClient::new("gem-key", "gemini-decision-1").with_base_url(format!("http://{addr}/v1beta"));
        let req = DecisionRequest::new("cargo test")
            .with_noul("is_safe", "Is this safe?")
            .with_choice(
                "risk",
                "Category?",
                BTreeMap::from([
                    ("safe_local".into(), "Safe local work".into()),
                    ("destructive".into(), "Destructive".into()),
                ]),
            );
        let res = client.decide(req).await.unwrap();
        assert_eq!(res.answers["is_safe"].as_noul(), Some(0.96));
        assert_eq!(res.answers["risk"].as_choice(), Some("safe_local"));
        assert_eq!(res.usage.input_tokens, 120);
        assert_eq!(res.usage.output_tokens, 0);

        let raw = server.await.unwrap();
        assert!(raw.contains("POST /v1beta/decisions"));
        assert!(raw.to_ascii_lowercase().contains("x-goog-api-key: gem-key"));
        assert!(raw.contains("\"model\":\"gemini-decision-1\""));
        assert!(raw.contains("\"input\":\"cargo test\""));
    }
}
