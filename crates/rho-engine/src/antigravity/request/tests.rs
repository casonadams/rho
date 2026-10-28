//! Unit tests for Antigravity request building, model mapping, and schema normalization.

mod envelope;
mod model;

use super::*;
use crate::provider::adapter::ModelCompletionRequest;
use rho_harness_core::model::ChatMessage;

fn minimal_request(history: Vec<ChatMessage>) -> ModelCompletionRequest {
    ModelCompletionRequest {
        preamble: Some("system prompt".to_string()),
        prompt: String::new(),
        messages: history,
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
    }
}

fn envelope() -> Envelope {
    Envelope {
        request_id: "agent/test-id/123/traj/2".to_string(),
        session_id: "42".to_string(),
    }
}

fn target<'a>(project: &'static str, runtime_model: &'a str) -> RequestTarget<'a> {
    RequestTarget {
        project,
        runtime_model,
        effort: Effort::Off,
    }
}

fn high_target<'a>(project: &'static str, runtime_model: &'a str) -> RequestTarget<'a> {
    RequestTarget {
        project,
        runtime_model,
        effort: Effort::High,
    }
}
