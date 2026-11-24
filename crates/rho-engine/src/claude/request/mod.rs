//! Claude Messages API request serialization.

mod contents;
#[cfg(test)]
mod tests;

pub use contents::convert_messages;

use super::http::ClaudeAuth;
use rho_harness_core::thinking::ThinkingLevel;

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

const CANONICAL_TOOLS: [(&str, &str); 10] = [
    ("read", "read"),
    ("write", "write"),
    ("edit", "edit"),
    ("bash", "bash"),
    ("glob", "glob"),
    ("grep", "grep"),
    ("webfetch", "web_fetch"),
    ("web_fetch", "web_fetch"),
    ("websearch", "web_search"),
    ("web_search", "web_search"),
];

pub fn from_claude_tool_name(name: &str) -> &str {
    if let Some(rest) = name.strip_prefix("mcp__rho__") {
        return rest;
    }
    for (candidate, canonical) in CANONICAL_TOOLS {
        if name.eq_ignore_ascii_case(candidate) {
            return canonical;
        }
    }
    name
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

use crate::provider::adapter::ModelCompletionRequest;
use rho_harness_core::config::CacheRetention;
use rho_harness_core::error::AppError;
use serde_json::{Value, json};

pub fn normalize_model_alias(model: &str) -> &str {
    match model {
        "default" | "sonnet" | "claude-sonnet-4-6" => "claude-sonnet-4-6",
        "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929",
        "opus" | "claude-opus-4-6" => "claude-opus-4-6",
        "haiku" | "claude-haiku-4-5" => "claude-haiku-4-5",
        "sonnet-5" | "claude-sonnet-5" => "claude-sonnet-5",
        "opus-5" | "claude-opus-5" => "claude-opus-5",
        "opus-5-5" | "opus-5.5" | "claude-opus-5.5" | "claude-opus-5-5" => "claude-opus-5-5",
        "sonnet-5-5" | "sonnet-5.5" | "claude-sonnet-5.5" | "claude-sonnet-5-5" => "claude-sonnet-5-5",
        "haiku-5" | "claude-haiku-5" | "haiku-5-5" | "haiku-5.5" | "claude-haiku-5.5" | "claude-haiku-5-5" => {
            "claude-haiku-5-5"
        }
        "fable" | "claude-fable" | "claude-fable-5" | "claude-fable-5.1" | "claude-fable-5-1" => "claude-fable-5-1",
        "mythos" | "claude-mythos" | "claude-mythos-5" | "claude-mythos-5.1" | "claude-mythos-5-1" => {
            "claude-mythos-5-1"
        }
        other => other,
    }
}

pub fn resolve_thinking_budget(level: Option<&str>) -> Option<u64> {
    match ThinkingLevel::from_setting(level) {
        ThinkingLevel::Off => None,
        ThinkingLevel::Minimal => Some(1024),
        ThinkingLevel::Low => Some(2048),
        ThinkingLevel::Medium => Some(4096),
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => Some(16384),
    }
}

pub fn resolve_effort(level: Option<&str>) -> Option<&'static str> {
    match ThinkingLevel::from_setting(level) {
        ThinkingLevel::Off => None,
        ThinkingLevel::Minimal | ThinkingLevel::Low => Some("low"),
        level => Some(level.as_str()),
    }
}

pub fn is_adaptive_model(model: &str) -> bool {
    is_at_least(model, 4, 6)
}

/// Claude 4.7+ and Mythos/Fable reject a non-default `temperature`, `top_p` or `top_k` with a 400 error
/// (Anthropic Messages API docs, sampling parameters).
fn rejects_sampling_params(model: &str) -> bool {
    is_at_least(model, 4, 7)
}

/// Fable and Mythos count as newer than every numbered Claude version.
fn is_at_least(model: &str, major: u32, minor: u32) -> bool {
    let lower = model.to_ascii_lowercase();
    if lower.contains("fable") || lower.contains("mythos") {
        return true;
    }
    let tokens: Vec<&str> = lower.split(['-', '.']).collect();
    tokens.iter().enumerate().find_map(|(idx, token)| {
        let found = token.parse::<u32>().ok()?;
        let found_minor = tokens
            .get(idx + 1)
            .filter(|t| t.len() <= 2)
            .and_then(|t| t.parse::<u32>().ok())
            .unwrap_or(0);
        Some((found, found_minor) >= (major, minor))
    }) == Some(true)
}

pub fn is_always_on_thinking_model(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("opus-5-5") || lower.contains("opus-5.5") || lower.contains("fable") || lower.contains("mythos")
}

/// Claude Sonnet 5.5 rejects `disabled`; `between_tools` is its lowest setting (Anthropic thinking docs).
fn turns_off_with_between_tools(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.contains("sonnet-5-5") || lower.contains("sonnet-5.5")
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

fn attach_disabled_thinking(body: &mut Value, model: &str, temp: Option<f64>) {
    if is_always_on_thinking_model(model) {
        body["output_config"] = json!({ "effort": "low" });
    } else if turns_off_with_between_tools(model) {
        body["thinking"] = json!({ "type": "between_tools" });
    } else {
        body["thinking"] = json!({ "type": "disabled" });
        if let Some(temperature) = temp {
            body["temperature"] = json!(temperature);
        }
    }
}

fn attach_enabled_adaptive_thinking(body: &mut Value, thinking_level: Option<&str>) {
    body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
    let effort = resolve_effort(thinking_level).unwrap_or("medium");
    body["output_config"] = json!({ "effort": effort });
}

fn attach_adaptive_thinking(body: &mut Value, model: &str, thinking_level: Option<&str>, temp: Option<f64>) {
    let is_off = thinking_level
        .map(|lvl| lvl.trim().eq_ignore_ascii_case("off"))
        .unwrap_or(false);
    if is_off {
        attach_disabled_thinking(body, model, temp);
    } else if thinking_level.is_some() || is_thinking_on_by_default(model) {
        attach_enabled_adaptive_thinking(body, thinking_level);
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

fn attach_tools_and_choice(body: &mut Value, request: &ModelCompletionRequest) {
    if !request.tools.is_empty() {
        body["tools"] = json!(convert_tools(request));
    }
}

pub fn build_request_body(
    model: &str,
    thinking_level: Option<&str>,
    request: &ModelCompletionRequest,
    auth: ClaudeAuth,
    cache_retention: CacheRetention,
) -> Result<Value, AppError> {
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

    let temperature = request
        .temperature
        .filter(|_| !rejects_sampling_params(normalized_model));
    attach_thinking_or_temp(
        &mut body,
        normalized_model,
        thinking_level,
        thinking_budget,
        temperature,
    );
    let mut system_blocks = Vec::new();
    if auth == ClaudeAuth::Login {
        system_blocks.push(json!({
            "type": "text",
            "text": "You are Claude Code, Anthropic's official CLI for Claude.",
        }));
    }
    if let Some(system) = system_prompt(request) {
        let sanitized = sanitize_system_prompt(&system) + &renamed_tools_note(request);
        system_blocks.push(json!({
            "type": "text",
            "text": sanitized,
        }));
    }
    if !system_blocks.is_empty() {
        body["system"] = json!(system_blocks);
    }
    attach_tools_and_choice(&mut body, request);
    mark_cache_breakpoints(&mut body, cache_retention.resolve(auth == ClaudeAuth::Login));
    Ok(body)
}

fn count_array_breakpoints(items: Option<&Vec<Value>>) -> usize {
    items.map_or(0, |arr| {
        arr.iter().filter(|item| item.get("cache_control").is_some()).count()
    })
}

fn count_message_content_breakpoints(messages: Option<&Vec<Value>>) -> usize {
    messages.map_or(0, |msgs| {
        msgs.iter()
            .filter_map(|msg| msg.get("content").and_then(Value::as_array))
            .flat_map(|parts| parts.iter())
            .filter(|part| part.get("cache_control").is_some())
            .count()
    })
}

/// Anthropic prompt caching: up to 4 breakpoints across tools, system, turn N-1, and messages tail.
pub fn count_cache_breakpoints(body: &Value) -> usize {
    let system = count_array_breakpoints(body.get("system").and_then(Value::as_array));
    let tools = count_array_breakpoints(body.get("tools").and_then(Value::as_array));
    let messages = count_message_content_breakpoints(body.get("messages").and_then(Value::as_array));
    system + tools + messages
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

fn last_cacheable_part(parts: &mut [Value]) -> Option<&mut Value> {
    parts.iter_mut().rev().find(|part| {
        !matches!(
            part.get("type").and_then(Value::as_str),
            Some("thinking" | "redacted_thinking")
        )
    })
}

fn cache_control(retention: CacheRetention) -> Value {
    match retention {
        CacheRetention::Long => json!({ "type": "ephemeral", "ttl": "1h" }),
        CacheRetention::Auto | CacheRetention::Short => json!({ "type": "ephemeral" }),
    }
}

fn last_cacheable_content(message: &mut Value) -> Option<&mut Value> {
    message
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .and_then(|parts| last_cacheable_part(parts))
}

fn last_block<'a>(body: &'a mut Value, field: &str) -> Option<&'a mut Value> {
    body.get_mut(field)
        .and_then(Value::as_array_mut)
        .and_then(|items| items.last_mut())
}

fn mark_breakpoint(target: Option<&mut Value>, control: &Value, count: &mut usize) {
    if *count < MAX_CACHE_BREAKPOINTS
        && let Some(target) = target
        && target.get("cache_control").is_none()
    {
        target["cache_control"] = control.clone();
        *count += 1;
    }
}

/// Anthropic allows at most four `cache_control` breakpoints per request.
const MAX_CACHE_BREAKPOINTS: usize = 4;

pub fn mark_cache_breakpoints(body: &mut Value, retention: CacheRetention) {
    let control = cache_control(retention);
    let mut count = count_cache_breakpoints(body);
    mark_breakpoint(last_block(body, "system"), &control, &mut count);
    mark_breakpoint(last_block(body, "tools"), &control, &mut count);

    let has_tools = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty());
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let turn_n_start_idx = messages
        .iter()
        .rposition(|msg| msg.get("role").and_then(Value::as_str) == Some("user") && !is_tool_result_message(msg))
        .unwrap_or(0);
    if turn_n_start_idx > 0 {
        let checkpoint = messages.get_mut(turn_n_start_idx - 1).and_then(last_cacheable_content);
        mark_breakpoint(checkpoint, &control, &mut count);
    }
    if !has_tools || messages.len() > 1 {
        mark_breakpoint(
            messages.last_mut().and_then(last_cacheable_content),
            &control,
            &mut count,
        );
    }
}

fn system_prompt(request: &ModelCompletionRequest) -> Option<String> {
    request.preamble.clone()
}

fn renamed_tools_note(request: &ModelCompletionRequest) -> String {
    let mut names: Vec<&str> = request.tools.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    let renamed: Vec<String> = names
        .into_iter()
        .filter_map(|name| {
            let sent = to_claude_tool_name(name);
            (sent != name).then(|| format!("`{name}` as `{sent}`"))
        })
        .collect();
    let Some((last, rest)) = renamed.split_last() else {
        return String::new();
    };
    let listed = if rest.is_empty() {
        last.clone()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    format!("\n\nThese instructions name tools by their short names; call {listed}.")
}

fn convert_tools(request: &ModelCompletionRequest) -> Vec<Value> {
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
