use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneQuestion {
    Choice {
        instructions: String,
        options: BTreeMap<String, String>,
    },
    Noul {
        instructions: String,
        criteria_true: String,
        criteria_false: String,
    },
    Score {
        instructions: String,
        levels: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneRequest {
    pub state: String,
    pub questions: BTreeMap<String, SystemOneQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    pub answer: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoulAnswer {
    pub answer: bool,
    pub probability: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SystemOneAnswer {
    Choice(ChoiceAnswer),
    Noul(NoulAnswer),
    Other(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneResponse {
    pub answers: BTreeMap<String, SystemOneAnswer>,
}

pub struct SystemOneClient {
    client: reqwest::Client,
    endpoint: String,
}

impl SystemOneClient {
    pub fn new(base_url: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        let endpoint = format!("{base}/v1/systemone");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client, endpoint }
    }

    pub async fn decide(&self, req: &SystemOneRequest) -> Result<SystemOneResponse, String> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let res = self
            .client
            .post(&self.endpoint)
            .headers(headers)
            .json(req)
            .send()
            .await
            .map_err(|e| format!("SystemOne request failed: {e}"))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(format!("SystemOne responded with {status}: {body}"));
        }

        res.json::<SystemOneResponse>()
            .await
            .map_err(|e| format!("Failed to parse SystemOne response: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_systemone_request_serialization() {
        let mut questions = BTreeMap::new();
        let mut options = BTreeMap::new();
        options.insert("smol".to_string(), "Quick typo or minor single-line edit".to_string());
        options.insert("slow".to_string(), "Complex architecture refactoring".to_string());
        questions.insert(
            "route".to_string(),
            SystemOneQuestion::Choice {
                instructions: "Select the most appropriate model tier".to_string(),
                options,
            },
        );

        let req = SystemOneRequest {
            state: "Fix typo in README".to_string(),
            questions,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"choice\""));
        assert!(json.contains("Fix typo in README"));
    }
}
