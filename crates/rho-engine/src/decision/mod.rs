//! Abstracted System-1 / structured decision models (`DecisionModel`).
//!
//! First-class native backends:
//! - Google Gemini Decisions API (`POST /v1beta/decisions`, e.g. `gemini/gemini-decision-1`)
//! - TypeSafe System One (`POST /v1/systemone`, e.g. `typesafe/jev-latest`)
//! - OpenRouter Decisions (`POST /api/alpha/decisions`, e.g. `openrouter/~typesafe/jev-latest`)
//!
//! Any standard chat `ModelHandle` (local Ollama, Flash-Lite, Haiku, etc.) can also be
//! wrapped in [`ChatDecisionAdapter`] so callers code against one [`DecisionModel`] trait.

pub mod chat;
pub mod gemini;
pub mod systemone;
pub mod types;

pub use chat::ChatDecisionAdapter;
pub use gemini::GeminiDecisionClient;
pub use systemone::SystemOneClient;
pub use types::{
    DecisionAnswer, DecisionHandle, DecisionInput, DecisionModel, DecisionPart, DecisionQuestion, DecisionRequest,
    DecisionResponse, DecisionUsage,
};

use crate::http::post_json_checked;
use crate::provider::error::HttpFailure;
use reqwest::header::HeaderMap;
use rho_harness_core::error::AppError;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// True when `(provider, model)` identifies a native structured-decision endpoint rather
/// than a chat completion model.
pub fn is_native_decision_model(provider: &str, model: &str) -> bool {
    let p = provider.to_ascii_lowercase();
    let m = model.to_ascii_lowercase();
    match p.as_str() {
        "gemini" | "google" => m.contains("decision"),
        "typesafe" => true,
        "openrouter" => m.contains("decision") || m.contains("jev"),
        _ => false,
    }
}

pub(super) async fn post_decision<T: DeserializeOwned>(
    endpoint: &str,
    headers: Result<HeaderMap, HttpFailure>,
    body: &Value,
    provider: &str,
    friendly_error: fn(Option<u16>, &str) -> String,
) -> Result<T, AppError> {
    let headers = headers.map_err(|f| f.into_app_error(friendly_error))?;
    let response = post_json_checked(endpoint, headers, body)
        .await
        .map_err(|f| f.into_app_error(friendly_error))?;
    let text = response
        .text()
        .await
        .map_err(|e| AppError::Network(format!("{provider} response read failed: {e}")))?;
    serde_json::from_str(&text).map_err(|e| AppError::Provider(format!("Invalid {provider} JSON: {e}")))
}
