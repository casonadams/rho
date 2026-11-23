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
