//! Claude Messages API request serialization.

mod contents;
#[cfg(test)]
mod tests;

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
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "read" => "Read".to_string(),
        "write" => "Write".to_string(),
        "edit" => "Edit".to_string(),
        "bash" => "Bash".to_string(),
        "grep" => "Grep".to_string(),
        "glob" => "Glob".to_string(),
        "webfetch" | "web_fetch" => "WebFetch".to_string(),
        "websearch" | "web_search" => "WebSearch".to_string(),
        "askuserquestion" => "AskUserQuestion".to_string(),
        "enterplanmode" => "EnterPlanMode".to_string(),
        "exitplanmode" => "ExitPlanMode".to_string(),
        "killshell" => "KillShell".to_string(),
        "notebookedit" => "NotebookEdit".to_string(),
        "skill" => "Skill".to_string(),
        "task" => "Task".to_string(),
        "taskoutput" => "TaskOutput".to_string(),
        "todowrite" => "TodoWrite".to_string(),
        _ if name.starts_with("mcp__") => name.to_string(),
        _ => format!("mcp__rho__{name}"),
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
    } else if name.eq_ignore_ascii_case("glob") {
        "glob"
    } else if name.eq_ignore_ascii_case("grep") {
        "grep"
    } else if name.eq_ignore_ascii_case("webfetch") || name.eq_ignore_ascii_case("web_fetch") {
        "web_fetch"
    } else if name.eq_ignore_ascii_case("websearch") || name.eq_ignore_ascii_case("web_search") {
        "web_search"
    } else {
        name
    }
}

fn sanitize_system_prompt(text: &str) -> String {
    text.replace(
        "operating inside rho, a coding agent harness",
        "operating inside the cli",
    )
    .replace("inside rho, a coding agent harness", "inside the cli")
    .replace("inside rho", "inside the cli")
    .replace("rho itself", "the cli itself")
    .replace("rho packages", "cli packages")
}

use rig::completion::{CompletionError, CompletionRequest};
use rig::message::{Message, ToolChoice};
use serde_json::{Value, json};

pub fn normalize_model_alias(model: &str) -> &str {
    match model {
        "default" | "sonnet" | "claude-sonnet-4-6" => "claude-sonnet-4-6",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250514",
        "opus" | "claude-opus-4-6" => "claude-opus-4-6",
        "haiku" | "claude-haiku-4-5" => "claude-haiku-4-5",
        "sonnet-5" | "claude-sonnet-5" => "claude-sonnet-5",
        "opus-5" | "claude-opus-5" => "claude-opus-5",
        "opus-5-5" | "opus-5.5" | "claude-opus-5.5" | "claude-opus-5-5" => "claude-opus-5-5",
        "fable" | "claude-fable" | "claude-fable-5" | "claude-fable-5.1" | "claude-fable-5-1" => "claude-fable-5-1",
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

pub fn resolve_effort(level: Option<&str>) -> Option<&'static str> {
    match level.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" => Some("low"),
        "medium" => Some("medium"),
        "high" => Some("high"),
        "xhigh" => Some("xhigh"),
        "max" => Some("max"),
        _ => None,
    }
}

pub fn is_adaptive_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    if lower.contains("fable") || lower.contains("mythos") {
        return true;
    }
    let tokens: Vec<&str> = lower.split('-').collect();
    for (idx, token) in tokens.iter().enumerate() {
        if let Ok(major) = token.parse::<u32>() {
            let minor = tokens.get(idx + 1).and_then(|t| t.parse::<u32>().ok()).unwrap_or(0);
            return major > 4 || (major == 4 && minor >= 6);
        }
    }
    false
}

pub fn is_always_on_thinking_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-5-5") || lower.contains("opus-5.5") || lower.contains("fable") || lower.contains("mythos")
}

pub fn is_thinking_on_by_default(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    if lower.contains("fable") || lower.contains("mythos") {
        return true;
    }
    let tokens: Vec<&str> = lower.split('-').collect();
    for token in tokens {
        if let Ok(major) = token.parse::<u32>() {
            return major >= 5;
        }
    }
    false
}

pub fn is_unsupported_forced_tool_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-5-5") || lower.contains("opus-5.5") || lower.contains("fable") || lower.contains("mythos")
}

fn adaptive_max_tokens(effort: Option<&str>) -> u64 {
    match effort {
        Some("xhigh" | "max") => 32768,
        _ => 16384,
    }
}

fn calculate_max_tokens(
    max_tokens: Option<u64>,
    thinking_budget: Option<u64>,
    is_adaptive: bool,
    effort: Option<&str>,
) -> u64 {
    if let Some(max) = max_tokens {
        return thinking_budget.map_or(max, |b| max.max(b + 1024));
    }
    if is_adaptive {
        return adaptive_max_tokens(effort);
    }
    thinking_budget.map_or(8192, |b| (b + 4096).max(8192))
}

fn attach_adaptive_thinking(body: &mut Value, model: &str, thinking_level: Option<&str>, temp: Option<f64>) {
    let is_off = thinking_level
        .map(|lvl| lvl.trim().eq_ignore_ascii_case("off"))
        .unwrap_or(false);
    if is_off {
        if is_always_on_thinking_model(model) {
            body["output_config"] = json!({ "effort": "low" });
        } else {
            body["thinking"] = json!({ "type": "disabled" });
            if let Some(temperature) = temp {
                body["temperature"] = json!(temperature);
            }
        }
    } else if thinking_level.is_some() || is_thinking_on_by_default(model) {
        body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
        let default_effort =
            if is_always_on_thinking_model(model) && (model.contains("opus-5-5") || model.contains("opus-5.5")) {
                "medium"
            } else {
                "high"
            };
        let effort = resolve_effort(thinking_level).unwrap_or(default_effort);
        body["output_config"] = json!({ "effort": effort });
    } else if let Some(temperature) = temp {
        body["temperature"] = json!(temperature);
    }
}

fn attach_thinking_or_temp(
    body: &mut Value,
    model: &str,
    thinking_level: Option<&str>,
    budget: Option<u64>,
    temp: Option<f64>,
) {
    if is_adaptive_model(model) {
        attach_adaptive_thinking(body, model, thinking_level, temp);
    } else if let Some(b) = budget {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": b });
    } else if let Some(temperature) = temp {
        body["temperature"] = json!(temperature);
    }
}

fn attach_tools_and_choice(body: &mut Value, request: &CompletionRequest, model: &str) {
    if !request.tools.is_empty() {
        body["tools"] = json!(convert_tools(request));
        if let Some(ref choice) = request.tool_choice {
            body["tool_choice"] = convert_tool_choice(choice, !is_unsupported_forced_tool_model(model));
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
    let adaptive = is_adaptive_model(normalized_model);
    let effort = resolve_effort(thinking_level);
    let max_tokens = calculate_max_tokens(request.max_tokens, thinking_budget, adaptive, effort);

    let mut body = json!({
        "model": normalized_model,
        "max_tokens": max_tokens,
        "messages": convert_messages(request),
        "stream": true,
    });

    attach_thinking_or_temp(
        &mut body,
        normalized_model,
        thinking_level,
        thinking_budget,
        request.temperature,
    );
    let mut system_blocks = vec![json!({
        "type": "text",
        "text": "You are Claude Code, Anthropic's official CLI for Claude.",
    })];
    if let Some(system) = system_prompt(request) {
        let sanitized = sanitize_system_prompt(&system);
        system_blocks.push(json!({
            "type": "text",
            "text": sanitized,
        }));
    }
    if let Some(last_block) = system_blocks.last_mut() {
        last_block["cache_control"] = json!({ "type": "ephemeral" });
    }
    body["system"] = json!(system_blocks);
    attach_tools_and_choice(&mut body, request, normalized_model);
    mark_cache_breakpoints(&mut body);
    Ok(body)
}

/// Anthropic prompt caching: up to 4 breakpoints across tools, system, turn N-1, and messages tail.
pub fn count_cache_breakpoints(body: &Value) -> usize {
    let mut count = 0;
    if let Some(system) = body.get("system").and_then(Value::as_array) {
        for block in system {
            if block.get("cache_control").is_some() {
                count += 1;
            }
        }
    }
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            if tool.get("cache_control").is_some() {
                count += 1;
            }
        }
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for msg in messages {
            if let Some(parts) = msg.get("content").and_then(Value::as_array) {
                for part in parts {
                    if part.get("cache_control").is_some() {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn is_tool_result_message(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
        })
        .unwrap_or(false)
}

pub fn mark_cache_breakpoints(body: &mut Value) {
    let mut count = count_cache_breakpoints(body);

    if count < 4
        && let Some(last_system) = body
            .get_mut("system")
            .and_then(Value::as_array_mut)
            .and_then(|blocks| blocks.last_mut())
        && last_system.get("cache_control").is_none()
    {
        last_system["cache_control"] = json!({ "type": "ephemeral" });
        count += 1;
    }

    if count < 4
        && let Some(last_tool) = body
            .get_mut("tools")
            .and_then(Value::as_array_mut)
            .and_then(|tools| tools.last_mut())
        && last_tool.get("cache_control").is_none()
    {
        last_tool["cache_control"] = json!({ "type": "ephemeral" });
        count += 1;
    }

    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    if messages.is_empty() {
        return;
    }

    let turn_n_start_idx = messages
        .iter()
        .rposition(|msg| msg.get("role").and_then(Value::as_str) == Some("user") && !is_tool_result_message(msg))
        .unwrap_or(0);

    if turn_n_start_idx > 0
        && count + 2 <= 4
        && let Some(checkpoint_msg) = messages.get_mut(turn_n_start_idx - 1)
        && let Some(parts) = checkpoint_msg.get_mut("content").and_then(Value::as_array_mut)
        && let Some(last_part) = parts.last_mut()
        && last_part.get("cache_control").is_none()
    {
        last_part["cache_control"] = json!({ "type": "ephemeral" });
        count += 1;
    }

    if count < 4
        && let Some(last_message) = messages.last_mut()
        && let Some(parts) = last_message.get_mut("content").and_then(Value::as_array_mut)
        && let Some(last_part) = parts.last_mut()
        && last_part.get("cache_control").is_none()
    {
        last_part["cache_control"] = json!({ "type": "ephemeral" });
        count += 1;
    }

    debug_assert!(
        count <= 4,
        "Anthropic allows at most 4 cache_control breakpoints, got {count}"
    );
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
    let mut tools = request.tools.clone();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
        .into_iter()
        .map(|t| {
            json!({
                "name": to_claude_tool_name(&t.name),
                "description": t.description,
                "input_schema": t.parameters,
            })
        })
        .collect()
}

fn convert_tool_choice(choice: &ToolChoice, forced_tools_supported: bool) -> Value {
    match choice {
        ToolChoice::Auto => json!({ "type": "auto" }),
        ToolChoice::Required => {
            if forced_tools_supported {
                json!({ "type": "any" })
            } else {
                json!({ "type": "auto" })
            }
        }
        ToolChoice::Specific { function_names } => {
            if forced_tools_supported {
                if let Some(name) = function_names.first() {
                    json!({ "type": "tool", "name": to_claude_tool_name(name) })
                } else {
                    json!({ "type": "auto" })
                }
            } else {
                json!({ "type": "auto" })
            }
        }
        ToolChoice::None => json!({ "type": "none" }),
    }
}
