//! Core domain types and trait for structured System-1 decision models.

use rho_harness_core::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub type DecisionHandle = Arc<dyn DecisionModel>;

/// Shared input evaluated by every question in a [`DecisionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionInput {
    Text(String),
    Parts(Vec<DecisionPart>),
}

impl DecisionInput {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// Flattens text parts into a single string for text-only backends.
    pub fn flatten_text(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Parts(parts) => parts
                .iter()
                .filter_map(|p| match p {
                    DecisionPart::Text(t) => Some(t.as_str()),
                    DecisionPart::ImageBase64 { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionPart {
    Text(String),
    ImageBase64 { data: String, mime_type: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionQuestion {
    /// Yes/no probability (`noul` in `[0.0, 1.0]`).
    Noul {
        instructions: String,
        criteria_true: Option<String>,
        criteria_false: Option<String>,
    },
    /// Single choice from a fixed map of `option_key -> description`.
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    /// Ordered rating across labeled levels (0..criteria.len()-1).
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionRequest {
    pub input: DecisionInput,
    pub questions: BTreeMap<String, DecisionQuestion>,
}

impl DecisionRequest {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: DecisionInput::Text(input.into()),
            questions: BTreeMap::new(),
        }
    }

    pub fn from_parts(parts: Vec<DecisionPart>) -> Self {
        Self {
            input: DecisionInput::Parts(parts),
            questions: BTreeMap::new(),
        }
    }

    pub fn with_noul(mut self, key: impl Into<String>, instructions: impl Into<String>) -> Self {
        self.questions.insert(
            key.into(),
            DecisionQuestion::Noul {
                instructions: instructions.into(),
                criteria_true: None,
                criteria_false: None,
            },
        );
        self
    }

    pub fn with_choice(
        mut self,
        key: impl Into<String>,
        instructions: impl Into<String>,
        criteria: BTreeMap<String, String>,
    ) -> Self {
        self.questions.insert(
            key.into(),
            DecisionQuestion::Choice {
                instructions: instructions.into(),
                criteria,
            },
        );
        self
    }

    pub fn with_score(
        mut self,
        key: impl Into<String>,
        instructions: impl Into<String>,
        criteria: Vec<String>,
    ) -> Self {
        self.questions.insert(
            key.into(),
            DecisionQuestion::Score {
                instructions: instructions.into(),
                criteria,
            },
        );
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DecisionAnswer {
    Noul {
        noul: f64,
        #[serde(default)]
        confidence: Option<f64>,
    },
    Choice {
        choice: String,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        confidence: Option<f64>,
    },
    Score {
        score: f64,
        #[serde(default)]
        legend: BTreeMap<String, String>,
        #[serde(default)]
        probabilities: BTreeMap<String, f64>,
        #[serde(default)]
        confidence: Option<f64>,
    },
    #[serde(other)]
    Other,
}

impl DecisionAnswer {
    pub fn as_noul(&self) -> Option<f64> {
        match self {
            Self::Noul { noul, .. } => Some(*noul),
            _ => None,
        }
    }

    pub fn as_choice(&self) -> Option<&str> {
        match self {
            Self::Choice { choice, .. } => Some(choice.as_str()),
            _ => None,
        }
    }

    pub fn as_score(&self) -> Option<f64> {
        match self {
            Self::Score { score, .. } => Some(*score),
            _ => None,
        }
    }

    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Noul { noul, confidence } => Some(confidence.unwrap_or_else(|| noul.max(1.0 - noul))),
            Self::Choice {
                choice,
                probabilities,
                confidence,
            } => confidence.or_else(|| probabilities.get(choice).copied()),
            Self::Score { confidence, .. } => *confidence,
            Self::Other => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecisionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DecisionResponse {
    pub answers: BTreeMap<String, DecisionAnswer>,
    pub usage: DecisionUsage,
}

#[async_trait::async_trait]
pub trait DecisionModel: Send + Sync + std::fmt::Debug {
    fn model_name(&self) -> &str;
    fn provider_name(&self) -> &str;
    /// True for dedicated decision APIs (Gemini Decisions, TypeSafe System One, OpenRouter Decisions).
    fn is_native_decision(&self) -> bool;

    async fn decide(&self, request: DecisionRequest) -> Result<DecisionResponse, AppError>;
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug)]
    pub struct MockDecisionModel {
        response: Mutex<Option<Result<DecisionResponse, AppError>>>,
        native: bool,
    }

    impl MockDecisionModel {
        pub fn native(response: DecisionResponse) -> Self {
            Self {
                response: Mutex::new(Some(Ok(response))),
                native: true,
            }
        }

        pub fn failing(err: AppError) -> Self {
            Self {
                response: Mutex::new(Some(Err(err))),
                native: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl DecisionModel for MockDecisionModel {
        fn model_name(&self) -> &str {
            "mock-decision"
        }

        fn provider_name(&self) -> &str {
            "mock"
        }

        fn is_native_decision(&self) -> bool {
            self.native
        }

        async fn decide(&self, _request: DecisionRequest) -> Result<DecisionResponse, AppError> {
            self.response
                .lock()
                .expect("mock lock")
                .take()
                .unwrap_or_else(|| Ok(DecisionResponse::default()))
        }
    }
}
