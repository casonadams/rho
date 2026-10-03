//! Unit tests for Antigravity request building, model mapping, and schema normalization.

mod envelope;
mod model;

use super::*;
use rig::completion::CompletionRequest;
use rig::message::Message;

fn minimal_request(history: Vec<Message>) -> CompletionRequest {
    let mut chat_history = vec![Message::system("system prompt")];
    chat_history.extend(history);
    CompletionRequest {
        model: None,
        chat_history,
        documents: Vec::new(),
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
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
