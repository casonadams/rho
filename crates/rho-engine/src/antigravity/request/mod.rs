//! Antigravity wire format, request side: the Gemini-shaped request envelope,
//! contents/tool conversion, and the public-model-id to runtime-id mapping.
//!
//! Mirrors pi-antigravity's proven request transport: the runtime ids are the
//! keys of `fetchAvailableModels`, Claude/GPT-OSS tool schemas go through the
//! legacy protobuf-allowlist `parameters` field, and unsigned tool calls are
//! flattened to user observations on Gemini 3+ replay.

pub mod contents;
pub mod model;
pub mod schema;

#[cfg(test)]
mod tests;

pub use contents::*;
pub use model::*;
pub use schema::*;

use rig::completion::{CompletionError, CompletionRequest};
use rig::message::Message;
use serde_json::{Value, json};

pub struct Envelope {
    pub request_id: String,
    pub session_id: String,
}

pub fn new_envelope() -> Envelope {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    Envelope {
        request_id: format!(
            "agent/{}/{}",
            uuid::Uuid::new_v4(),
            chrono::Utc::now().timestamp_millis()
        ),
        session_id: i64::from_le_bytes(bytes).to_string(),
    }
}

/// The routing facts a wire request needs: which Cloud Code Assist project to
/// bill against, which backend runtime model to invoke, and the thinking
/// effort that shaped the runtime pick.
#[derive(Clone, Copy)]
pub struct RequestTarget<'a> {
    pub project: &'a str,
    pub runtime_model: &'a str,
    pub effort: Effort,
}

/// Build the full Antigravity request envelope for a completion request.
pub fn build_request_body(
    target: RequestTarget<'_>,
    request: &CompletionRequest,
    envelope: &Envelope,
) -> Result<Value, CompletionError> {
    let runtime_model = target.runtime_model;
    let is_claude = runtime_model.starts_with("claude-");
    let legacy_parameters = is_claude || runtime_model.starts_with("gpt-oss-");

    let mut generation_config = json!({
        "maxOutputTokens": cap_max_tokens(runtime_model, request.max_tokens),
    });
    if let Some(temperature) = request.temperature {
        generation_config["temperature"] = json!(temperature);
    }
    let thinking = thinking_config(runtime_model, target.effort);
    if !thinking.is_null() {
        generation_config["thinkingConfig"] = thinking;
    }

    let used_claude = is_claude.to_string();
    let mut labels = json!({
        "last_step_index": "1",
        "trajectory_id": uuid::Uuid::new_v4().to_string(),
        "used_claude": used_claude,
        "used_claude_conservative": used_claude,
    });
    if let Some(enum_label) = model_enum_label(runtime_model) {
        labels["model_enum"] = json!(enum_label);
    }

    let mut gemini_request = json!({
        "contents": convert_contents(request, runtime_model),
        "sessionId": envelope.session_id,
        "labels": labels,
    });
    let system_prompt = system_prompt(request);
    gemini_request["systemInstruction"] = json!({
        "role": "user",
        "parts": [{ "text": system_prompt }],
    });
    gemini_request["generationConfig"] = generation_config;

    if !request.tools.is_empty() {
        let tools = convert_tools(request, legacy_parameters);
        gemini_request["tools"] = tools.expect("non-empty tools produce declarations");
        gemini_request["toolConfig"] = json!({
            "functionCallingConfig": { "mode": tool_config_mode(request.tool_choice.clone()) }
        });
    } else if is_claude {
        gemini_request["toolConfig"] = json!({
            "functionCallingConfig": { "mode": "VALIDATED" }
        });
    }

    Ok(json!({
        "project": target.project,
        "model": runtime_model,
        "request": gemini_request,
        "requestType": "agent",
        "userAgent": "antigravity",
        "requestId": envelope.request_id,
    }))
}

fn system_prompt(request: &CompletionRequest) -> String {
    const DEFAULT_INSTRUCTION: &str = "You are Antigravity, a powerful agentic AI coding assistant designed by Google DeepMind. You are pair programming with a user to solve coding tasks. Be concise, practical, and tool-aware.";
    for message in &request.chat_history {
        if let Message::System { content } = message {
            return content.clone();
        }
    }
    request
        .preamble
        .clone()
        .unwrap_or_else(|| DEFAULT_INSTRUCTION.to_string())
}
