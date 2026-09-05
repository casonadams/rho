//! Claude Messages API request serialization.

mod contents;

pub use contents::convert_messages;

use rig::completion::{CompletionError, CompletionRequest};
use rig::message::{Message, ToolChoice};
use serde_json::{Value, json};

pub fn normalize_model_alias(model: &str) -> &str {
    match model {
        "default" | "sonnet" | "claude-sonnet-4-5" => "claude-sonnet-4-5-20250514",
        "opus" | "claude-opus-4-6" => "claude-opus-4-6",
        "haiku" | "claude-haiku-4-5" => "claude-haiku-4-5",
        other => other,
    }
}

pub fn resolve_thinking_budget(level: Option<&str>) -> Option<u64> {
    match level.unwrap_or("off").trim().to_ascii_lowercase().as_str() {
        "minimal" => Some(1024),
        "low" => Some(2048),
        "medium" => Some(4096),
        "high" | "xhigh" | "max" => Some(16384),
        _ => None,
    }
}

pub fn build_request_body(
    model: &str,
    thinking_level: Option<&str>,
    request: &CompletionRequest,
) -> Result<Value, CompletionError> {
    let normalized_model = normalize_model_alias(model);
    let thinking_budget = resolve_thinking_budget(thinking_level);
    let max_tokens = match (request.max_tokens, thinking_budget) {
        (Some(max), Some(budget)) => max.max(budget + 1024),
        (None, Some(budget)) => (budget + 4096).max(8192),
        (Some(max), None) => max,
        (None, None) => 8192,
    };

    let mut body = json!({
        "model": normalized_model,
        "max_tokens": max_tokens,
        "messages": convert_messages(request),
        "stream": true,
    });

    if let Some(budget) = thinking_budget {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
    } else if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }

    if let Some(system) = system_prompt(request) {
        body["system"] = json!(system);
    }

    if !request.tools.is_empty() {
        body["tools"] = json!(convert_tools(request));
        if let Some(ref choice) = request.tool_choice {
            body["tool_choice"] = convert_tool_choice(choice);
        }
    }

    Ok(body)
}

fn system_prompt(request: &CompletionRequest) -> Option<String> {
    for message in &request.chat_history {
        if let Message::System { content } = message {
            return Some(content.clone());
        }
    }
    request.preamble.clone()
}

fn convert_tools(request: &CompletionRequest) -> Vec<Value> {
    request
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

fn convert_tool_choice(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::Required => json!({ "type": "any" }),
        ToolChoice::Specific { function_names } => {
            if let Some(name) = function_names.first() {
                json!({ "type": "tool", "name": name })
            } else {
                json!({ "type": "auto" })
            }
        }
        ToolChoice::None => json!({ "type": "none" }),
    }
}
