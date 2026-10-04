use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneQuestion {
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    Noul {
        instructions: String,
        criteria_true: String,
        criteria_false: String,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemOneRequest {
    pub model: String,
    pub state: String,
    pub questions: BTreeMap<String, SystemOneQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    pub choice: String,
    #[serde(default)]
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoulAnswer {
    pub noul: f64,
}

impl NoulAnswer {
    pub fn is_safe(&self, threshold: f64) -> bool {
        self.noul >= threshold
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreAnswer {
    pub score: f64,
    #[serde(default)]
    pub legend: BTreeMap<String, String>,
    #[serde(default)]
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SystemOneAnswer {
    Choice(ChoiceAnswer),
    Noul(NoulAnswer),
    Score(ScoreAnswer),
    #[serde(other)]
    Other,
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
        let mut criteria = BTreeMap::new();
        criteria.insert("smol".to_string(), "Quick typo or minor single-line edit".to_string());
        criteria.insert("slow".to_string(), "Complex architecture refactoring".to_string());
        questions.insert(
            "route".to_string(),
            SystemOneQuestion::Choice {
                instructions: "Select the most appropriate model tier".to_string(),
                criteria,
            },
        );

        let req = SystemOneRequest {
            model: "clef-flash".to_string(),
            state: "Fix typo in README".to_string(),
            questions,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"choice\""));
        assert!(json.contains("\"model\":\"clef-flash\""));
        assert!(json.contains("Fix typo in README"));
    }

    #[test]
    fn test_systemone_response_deserialization() {
        let noul_raw = r#"{"model":"clef-flash","answers":{"is_safe":{"type":"noul","noul":0.957}},"usage":{"input_tokens":153,"output_tokens":0}}"#;
        let res: SystemOneResponse = serde_json::from_str(noul_raw).unwrap();
        match res.answers.get("is_safe").unwrap() {
            SystemOneAnswer::Noul(n) => {
                assert!(n.noul > 0.9);
                assert!(n.is_safe(0.9));
            }
            _ => panic!("expected noul answer"),
        }
    }
}
