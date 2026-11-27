//! TypeSafe System One (`POST /v1/systemone`) and OpenRouter Decisions (`POST /api/alpha/decisions`) client.

use super::post_decision;
use super::types::{DecisionAnswer, DecisionModel, DecisionQuestion, DecisionRequest, DecisionResponse, DecisionUsage};
use crate::http::bearer;
use crate::provider::error::{HttpFailure, error_message_or_excerpt};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use rho_harness_core::error::AppError;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const TYPESAFE_BASE_URL: &str = "https://api.typesafe.ai";
pub const OPENROUTER_DECISIONS_BASE_URL: &str = "https://openrouter.ai/api/alpha";

#[derive(Clone)]
pub struct SystemOneClient {
    provider: String,
    endpoint: String,
    api_key: String,
    model: String,
}

impl std::fmt::Debug for SystemOneClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemOneClient")
            .field("provider", &self.provider)
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl SystemOneClient {
    pub fn typesafe(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let base = std::env::var("TYPESAFE_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| TYPESAFE_BASE_URL.to_string());
        let endpoint = format!("{}/v1/systemone", base.trim_end_matches('/'));
        Self {
            provider: "typesafe".to_string(),
            endpoint,
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn openrouter(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        let endpoint = format!("{}/decisions", OPENROUTER_DECISIONS_BASE_URL.trim_end_matches('/'));
        Self {
            provider: "openrouter".to_string(),
            endpoint,
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    fn headers(&self) -> Result<HeaderMap, HttpFailure> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(AUTHORIZATION, bearer(&self.api_key)?);
        Ok(headers)
    }
}

fn serialize_question(question: &DecisionQuestion) -> Value {
    match question {
        DecisionQuestion::Noul {
            instructions,
            criteria_true,
            criteria_false,
        } => {
            let mut obj = Map::new();
            obj.insert("type".to_string(), json!("noul"));
            obj.insert("instructions".to_string(), json!(instructions));
            if let Some(t) = criteria_true {
                obj.insert("criteria_true".to_string(), json!(t));
            }
            if let Some(f) = criteria_false {
                obj.insert("criteria_false".to_string(), json!(f));
            }
            Value::Object(obj)
        }
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

fn build_request_body(model: &str, request: &DecisionRequest) -> Value {
    let mut questions = Map::new();
    for (key, question) in &request.questions {
        questions.insert(key.clone(), serialize_question(question));
    }
    json!({
        "model": model,
        "state": request.input.flatten_text(),
        "questions": Value::Object(questions),
    })
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
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
        Some(code) => format!("System One decision API error ({code}): {msg}"),
        None => format!("System One decision request failed: {msg}"),
    }
}

#[async_trait::async_trait]
impl DecisionModel for SystemOneClient {
    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }

    fn is_native_decision(&self) -> bool {
        true
    }

    async fn decide(&self, request: DecisionRequest) -> Result<DecisionResponse, AppError> {
        let body = build_request_body(&self.model, &request);
        let parsed: WireResponse =
            post_decision(&self.endpoint, self.headers(), &body, "System One", friendly_error).await?;
        let usage = parsed.usage.map_or_else(DecisionUsage::default, |u| DecisionUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
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

    #[tokio::test]
    async fn typesafe_client_sends_state_and_decodes_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let payload =
                r#"{"answers":{"ok":{"type":"noul","noul":0.88}},"usage":{"input_tokens":50,"output_tokens":0}}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            req
        });

        let client =
            SystemOneClient::typesafe("ts-key", "jev-latest").with_endpoint(format!("http://{addr}/v1/systemone"));
        let res = client
            .decide(DecisionRequest::new("hello").with_noul("ok", "Is this ok?"))
            .await
            .unwrap();
        assert_eq!(res.answers["ok"].as_noul(), Some(0.88));
        assert_eq!(res.usage.input_tokens, 50);

        let raw = server.await.unwrap();
        assert!(raw.contains("POST /v1/systemone"));
        assert!(raw.contains("\"state\":\"hello\""));
        assert!(raw.contains("\"model\":\"jev-latest\""));
    }
}
