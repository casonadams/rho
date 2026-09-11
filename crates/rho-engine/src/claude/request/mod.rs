//! Claude Messages API request serialization.

mod contents;

pub use contents::convert_messages;

pub fn is_core_claude_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "read"
            | "write"
            | "edit"
            | "bash"
            | "grep"
            | "glob"
            | "askuserquestion"
            | "enterplanmode"
            | "exitplanmode"
            | "killshell"
            | "notebookedit"
            | "skill"
            | "task"
            | "taskoutput"
            | "todowrite"
            | "webfetch"
            | "websearch"
    )
}

pub fn to_claude_tool_name(name: &str) -> String {
    if is_core_claude_tool(name) || name.starts_with("mcp__") {
        name.to_string()
    } else {
        format!("mcp__rho__{name}")
    }
}

pub fn from_claude_tool_name(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("mcp__rho__") {
        rest
    } else if name.eq_ignore_ascii_case("read") {
        "read"
    } else if name.eq_ignore_ascii_case("write") {
        "write"
    } else if name.eq_ignore_ascii_case("edit") {
        "edit"
    } else if name.eq_ignore_ascii_case("bash") {
        "bash"
    } else {
        name
    }
}

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

fn calculate_max_tokens(max_tokens: Option<u64>, thinking_budget: Option<u64>) -> u64 {
    match (max_tokens, thinking_budget) {
        (Some(max), Some(budget)) => max.max(budget + 1024),
        (None, Some(budget)) => (budget + 4096).max(8192),
        (Some(max), None) => max,
        (None, None) => 8192,
    }
}

fn attach_thinking_or_temp(body: &mut Value, budget: Option<u64>, temp: Option<f64>) {
    if let Some(b) = budget {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": b });
    } else if let Some(temperature) = temp {
        body["temperature"] = json!(temperature);
    }
}

fn attach_tools_and_choice(body: &mut Value, request: &CompletionRequest) {
    if !request.tools.is_empty() {
        body["tools"] = json!(convert_tools(request));
        if let Some(ref choice) = request.tool_choice {
            body["tool_choice"] = convert_tool_choice(choice);
        }
    }
}

pub fn build_request_body(
    model: &str,
    thinking_level: Option<&str>,
    request: &CompletionRequest,
) -> Result<Value, CompletionError> {
    let normalized_model = normalize_model_alias(model);
    let thinking_budget = resolve_thinking_budget(thinking_level);
    let max_tokens = calculate_max_tokens(request.max_tokens, thinking_budget);

    let mut body = json!({
        "model": normalized_model,
        "max_tokens": max_tokens,
        "messages": convert_messages(request),
        "stream": true,
    });

    attach_thinking_or_temp(&mut body, thinking_budget, request.temperature);
    if let Some(system) = system_prompt(request) {
        body["system"] = json!([{
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" },
        }]);
    }
    attach_tools_and_choice(&mut body, request);
    mark_cache_breakpoints(&mut body);
    Ok(body)
}

/// Prompt caching: one breakpoint per static prefix block (system prompt,
/// last tool) plus the conversation tail. The Messages API allows at most
/// four cache breakpoints per request; three are used here.
fn mark_cache_breakpoints(body: &mut Value) {
    if let Some(last_tool) = body
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .and_then(|tools| tools.last_mut())
    {
        last_tool["cache_control"] = json!({ "type": "ephemeral" });
    }
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    for message in messages.iter_mut().rev() {
        let Some(parts) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        if let Some(last_tool_result) = parts
            .iter_mut()
            .rev()
            .find(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            last_tool_result["cache_control"] = json!({ "type": "ephemeral" });
            return;
        }
    }
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
                "name": to_claude_tool_name(&t.name),
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
                json!({ "type": "tool", "name": to_claude_tool_name(name) })
            } else {
                json!({ "type": "auto" })
            }
        }
        ToolChoice::None => json!({ "type": "none" }),
    }
}
