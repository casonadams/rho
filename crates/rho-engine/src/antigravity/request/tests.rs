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
        messages: history.into(),
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
        thinking_level: None,
    }
}

#[test]
fn a_compaction_summary_in_the_history_reaches_the_model_as_user_text() {
    let request = minimal_request(vec![
        ChatMessage::System {
            content: "Summary of earlier work".into(),
        },
        ChatMessage::user("continue"),
    ]);

    let contents = contents::convert_contents(&request, "gemini-3-pro");

    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0].role, "user");
    let texts: Vec<&str> = contents[0].parts.iter().filter_map(|p| p.text.as_deref()).collect();
    assert_eq!(texts, ["Summary of earlier work", "continue"]);
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
