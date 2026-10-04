use std::collections::BTreeMap;
use std::time::Duration;

use rho_harness_core::config::Config;
use serde::{Deserialize, Serialize};

use crate::provider::systemone::{
    SystemOneAnswer, SystemOneClient, SystemOneQuestion, SystemOneRequest, SystemOneResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Smol,
    Standard,
    Slow,
}

impl ModelTier {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Smol => "smol",
            Self::Standard => "standard",
            Self::Slow => "slow",
        }
    }
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteVerdict {
    pub tier: ModelTier,
    pub model: String,
    pub confidence: Option<f64>,
    pub fallback: bool,
}

pub struct ModelRouter {
    client: SystemOneClient,
    model: String,
    timeout: Duration,
    smol: Option<String>,
    standard: String,
    slow: Option<String>,
}

impl ModelRouter {
    pub fn new(
        endpoint_or_base: &str,
        decision_model: &str,
        standard: String,
        smol: Option<String>,
        slow: Option<String>,
    ) -> Self {
        Self {
            client: SystemOneClient::new(endpoint_or_base),
            model: decision_model.to_string(),
            timeout: Duration::from_millis(150),
            smol,
            standard,
            slow,
        }
    }

    pub fn from_config(config: &Config) -> Option<Self> {
        let (base_url, decision_model) = crate::provider::resolve_judge_model(config)?;
        Some(Self::new(
            &base_url,
            &decision_model,
            config.model.clone(),
            config.smol_model().map(ToString::to_string),
            config.slow_model().map(ToString::to_string),
        ))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build_routing_request(decision_model: &str, prompt: &str) -> SystemOneRequest {
        let mut criteria = BTreeMap::new();
        criteria.insert(
            "smol".to_string(),
            "Simple typo fixes, single file modifications, formatting, git status inspection, small targeted queries."
                .to_string(),
        );
        criteria.insert(
            "standard".to_string(),
            "Normal feature development, bug investigations, multi-file edits, standard coding assistance.".to_string(),
        );
        criteria.insert(
            "slow".to_string(),
            "Complex architectural redesigns, concurrency issues, deep algorithmic debugging, difficult system-wide refactors.".to_string(),
        );

        let mut questions = BTreeMap::new();
        questions.insert(
            "tier".to_string(),
            SystemOneQuestion::Choice {
                instructions:
                    "Select the most appropriate model capability tier for the task based on complexity and risk."
                        .to_string(),
                criteria,
            },
        );

        SystemOneRequest {
            model: decision_model.to_string(),
            state: format!("User prompt to evaluate:\n{prompt}"),
            questions,
        }
    }

    fn resolve_tier_choice(&self, choice: &str) -> (ModelTier, String) {
        match choice.to_ascii_lowercase().as_str() {
            "smol" => match &self.smol {
                Some(model) => (ModelTier::Smol, model.clone()),
                None => (ModelTier::Standard, self.standard.clone()),
            },
            "slow" => match &self.slow {
                Some(model) => (ModelTier::Slow, model.clone()),
                None => (ModelTier::Standard, self.standard.clone()),
            },
            _ => (ModelTier::Standard, self.standard.clone()),
        }
    }

    pub fn parse_verdict(&self, response: &SystemOneResponse) -> Option<RouteVerdict> {
        match response.answers.get("tier") {
            Some(SystemOneAnswer::Choice(ans)) => {
                let (tier, model) = self.resolve_tier_choice(&ans.choice);
                Some(RouteVerdict {
                    tier,
                    model,
                    confidence: ans.confidence,
                    fallback: false,
                })
            }
            _ => None,
        }
    }

    pub async fn route_prompt(&self, prompt: &str) -> RouteVerdict {
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return RouteVerdict {
                tier: ModelTier::Standard,
                model: self.standard.clone(),
                confidence: None,
                fallback: false,
            };
        }

        let req = Self::build_routing_request(&self.model, trimmed);
        let query_future = self.client.decide(&req);

        match tokio::time::timeout(self.timeout, query_future).await {
            Ok(Ok(resp)) => self.parse_verdict(&resp).unwrap_or_else(|| RouteVerdict {
                tier: ModelTier::Standard,
                model: self.standard.clone(),
                confidence: None,
                fallback: true,
            }),
            _ => RouteVerdict {
                tier: ModelTier::Standard,
                model: self.standard.clone(),
                confidence: None,
                fallback: true,
            },
        }
    }
}

#[cfg(test)]
mod tests;
