//! Antigravity wire format, request side: the Gemini-shaped request envelope,
//! contents/tool conversion, and the public-model-id to runtime-id mapping.
//!
//! Mirrors pi-antigravity's proven request transport: the runtime ids are the
//! keys of `fetchAvailableModels`, Claude/GPT-OSS tool schemas go through the
//! legacy protobuf-allowlist `parameters` field, and unsigned tool calls are
//! flattened to user observations on Gemini 3+ replay.

use rig::completion::{CompletionError, CompletionRequest, ToolDefinition};
use rig::message::{AssistantContent, Message, ToolChoice, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Normalized thinking effort: off/minimal/low/medium/high. rho's xhigh/max
/// map to high (the backend advertises no finer level).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
}

impl Effort {
    /// Parse rho's thinking level (see `THINKING_LEVELS` in the REPL).
    pub fn parse(level: Option<&str>) -> Self {
        match level.unwrap_or("off").trim().to_ascii_lowercase().as_str() {
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" | "xhigh" | "max" => Self::High,
            _ => Self::Off,
        }
    }
}

/// Public selectable model ids + thinking effort → backend runtime ids.
/// Static table mirroring pi-antigravity's routing; unknown ids (including
/// already-runtime ids like `gemini-3.7-flash-high`) pass through untouched.
/// ponytail: static table — extend when Google advertises new families.
pub fn resolve_runtime_model(public_id: &str, effort: Effort) -> String {
    match (public_id, effort) {
        ("claude-opus-4-6", _) => "claude-opus-4-6-thinking".to_string(),
        ("gpt-oss-120b", _) => "gpt-oss-120b-medium".to_string(),
        ("gemini-3.5-flash", Effort::High) => "gemini-3-flash-agent".to_string(),
        ("gemini-3.5-flash", Effort::Medium) => "gemini-3.5-flash-low".to_string(),
        ("gemini-3.5-flash", _) => "gemini-3.5-flash-extra-low".to_string(),
        ("gemini-3.1-pro", Effort::High) => "gemini-pro-agent".to_string(),
        ("gemini-3.1-pro", _) => "gemini-3.1-pro-low".to_string(),
        (family @ ("gemini-3.8-flash" | "gemini-3.7-flash" | "gemini-3.6-flash"), effort) => {
            format!("{family}-{}", level_suffix(effort))
        }
        (other, _) => other.to_string(),
    }
}

fn level_suffix(effort: Effort) -> &'static str {
    match effort {
        Effort::Medium => "medium",
        Effort::High => "high",
        _ => "low",
    }
}

/// Collapse a runtime id into its public family id + advertised thinking
/// level (pi parity: tiered variants and agent aliases fold into one family).
/// Suffix order matters: extra-* must be stripped before plain low/high.
pub fn collapse_runtime_id(runtime: &str) -> (String, Option<Effort>) {
    match runtime {
        "gemini-3-flash-agent" => return ("gemini-3.5-flash".to_string(), Some(Effort::High)),
        "gemini-pro-agent" => return ("gemini-3.1-pro".to_string(), Some(Effort::High)),
        _ => {}
    }
    for (suffix, level) in [
        ("extra-low", Some(Effort::Low)),
        ("extra-high", Some(Effort::High)),
        ("thinking", Some(Effort::High)),
        ("minimal", Some(Effort::Minimal)),
        ("medium", Some(Effort::Medium)),
        ("high", Some(Effort::High)),
        ("low", Some(Effort::Low)),
        ("tiered", None),
    ] {
        if let Some(base) = runtime.strip_suffix(suffix)
            && base.ends_with('-')
        {
            return (base.trim_end_matches('-').to_string(), level);
        }
    }
    (runtime.to_string(), None)
}

/// Next-generation fallback when a runtime id 404s (pi parity: 3.8 → 3.7 → 3.6).
pub fn fallback_runtime_model(runtime: &str) -> Option<String> {
    if let Some(rest) = runtime.strip_prefix("gemini-3.8-flash-") {
        return Some(format!("gemini-3.7-flash-{rest}"));
    }
    if runtime == "gemini-3.8-flash" {
        return Some("gemini-3.7-flash-low".to_string());
    }
    if let Some(rest) = runtime.strip_prefix("gemini-3.7-flash-") {
        return Some(format!("gemini-3.6-flash-{rest}"));
    }
    if runtime == "gemini-3.7-flash" {
        return Some("gemini-3.6-flash-low".to_string());
    }
    None
}

fn max_output_tokens_cap(runtime: &str) -> u64 {
    if runtime.starts_with("claude-") {
        64000
    } else if runtime.starts_with("gpt-oss-") {
        32768
    } else if runtime.starts_with("gemini-3.1-pro") {
        65535
    } else if runtime.starts_with("gemini-") {
        65536
    } else {
        8192
    }
}

/// Verified backend caps per runtime id; requesting more returns 400.
fn cap_max_tokens(runtime: &str, requested: Option<u64>) -> u64 {
    let cap = max_output_tokens_cap(runtime);
    requested.map(|t| t.min(cap)).unwrap_or(cap)
}

fn gemini_requires_thought_signature(runtime: &str) -> bool {
    let Some(rest) = runtime.strip_prefix("gemini-") else {
        return false;
    };
    let major: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    major.parse::<u32>().map(|v| v >= 3).unwrap_or(true)
}

fn needs_function_call_id(runtime: &str) -> bool {
    runtime.starts_with("claude-") || runtime.starts_with("gpt-oss-")
}

pub fn sanitize_tool_call_id(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.chars().take(64).collect()
}

// ---------------------------------------------------------------- metadata

fn strip_meta_schema(schema: &Value) -> Value {
    const META_KEYS: [&str; 9] = [
        "$schema",
        "$id",
        "$anchor",
        "$dynamicAnchor",
        "$vocabulary",
        "$comment",
        "$defs",
        "definitions",
        "additionalProperties",
    ];
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(strip_meta_schema).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !META_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), strip_meta_schema(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Cloud Code Assist's Claude/GPT-OSS custom-tool bridge accepts only a
/// protobuf `Schema` subset; anything else 400s with `Unknown name`.
fn normalize_custom_tool_schema(schema: &Value) -> Value {
    const ALLOWED: [&str; 5] = ["type", "description", "properties", "required", "items"];
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(normalize_custom_tool_schema).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if key == "type" {
                    let t = match value {
                        Value::String(s) => Some(json!(s)),
                        Value::Array(items) => items
                            .iter()
                            .find(|v| v.is_string() && v.as_str() != Some("null"))
                            .cloned(),
                        _ => None,
                    };
                    if let Some(t) = t {
                        out.insert("type".into(), t);
                    }
                } else if key == "properties" && value.is_object() {
                    out.insert("properties".into(), normalize_custom_tool_schema(value));
                } else if ALLOWED.contains(&key.as_str()) {
                    out.insert(key.clone(), normalize_custom_tool_schema(value));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

// ----------------------------------------------------------- request build

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thought: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thought_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_data: Option<InlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<FunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_response: Option<FunctionResponse>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct FunctionCall {
    name: String,
    args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct FunctionResponse {
    name: String,
    response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
struct Content {
    role: String,
    parts: Vec<Part>,
}

fn part_text(text: impl Into<String>) -> Part {
    Part {
        text: Some(text.into()),
        ..Part::default()
    }
}

fn part_image(data: &str, media_type: Option<&str>) -> Option<Part> {
    // Accept both bare base64 and data URLs.
    let (mime, data) = match data.strip_prefix("data:") {
        Some(rest) => match rest.split_once(";base64,") {
            Some((m, d)) => (m.to_string(), d.to_string()),
            None => return None,
        },
        None => (media_type.unwrap_or("image/png").to_string(), data.to_string()),
    };
    if data.is_empty() {
        return None;
    }
    Some(Part {
        inline_data: Some(InlineData { mime_type: mime, data }),
        ..Part::default()
    })
}

fn append_turn(contents: &mut Vec<Content>, role: &str, parts: Vec<Part>) {
    if parts.is_empty() {
        return;
    }
    match contents.last_mut() {
        Some(last) if last.role == role => last.parts.extend(parts),
        _ => contents.push(Content {
            role: role.to_string(),
            parts,
        }),
    }
}

fn tool_result_text(content: &[rig::message::ToolResultContent]) -> String {
    content
        .iter()
        .map(|c| match c {
            rig::message::ToolResultContent::Text(text) => text.text.clone(),
            rig::message::ToolResultContent::Json { value } => value.to_string(),
            rig::message::ToolResultContent::Image(_) => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert rig chat history into Gemini `contents`, mirroring pi's replay
/// rules: unsigned tool calls on Gemini 3+ are dropped and their results are
/// replayed as user observations, because the backend validates thought
/// signatures on Gemini 3 function-call replay.
fn convert_contents(request: &CompletionRequest, runtime_model: &str) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();
    let mut dropped = std::collections::HashMap::new();
    let requires_sig = gemini_requires_thought_signature(runtime_model);
    let call_ids = needs_function_call_id(runtime_model);

    for message in &request.chat_history {
        match message {
            Message::System { .. } => {} // feeds systemInstruction, not contents
            Message::User { content } => {
                let mut parts = Vec::new();
                for item in content {
                    match item {
                        UserContent::Text(text) => {
                            if !text.text.trim().is_empty() {
                                parts.push(part_text(text.text.clone()));
                            }
                        }
                        UserContent::ToolResult(result) => {
                            let response_text = tool_result_text(&result.content);
                            let raw_id = result.call.to_string();
                            let sanitized_id = sanitize_tool_call_id(&raw_id);
                            let dropped_args = requires_sig
                                .then(|| dropped.get(&raw_id).or_else(|| dropped.get(&sanitized_id)).cloned())
                                .flatten();
                            if let Some(args) = dropped_args {
                                let label = if args == "{}" {
                                    format!("`{}`", result.name)
                                } else {
                                    format!("`{}` ({})", result.name, args)
                                };
                                parts.push(part_text(format!("[Observation from {label}:\n{response_text}]")));
                            } else {
                                parts.push(Part {
                                    function_response: Some(FunctionResponse {
                                        name: result.name.clone(),
                                        response: json!({ "output": response_text }),
                                        id: call_ids.then(|| sanitized_id.clone()),
                                    }),
                                    ..Part::default()
                                });
                            }
                        }
                        UserContent::Image(image) => {
                            let data = image_data(&image.data);
                            if let Some(data) = data {
                                let media_type = image.media_type.as_ref().map(|m| match m {
                                    rig::message::ImageMediaType::JPEG => "image/jpeg",
                                    rig::message::ImageMediaType::PNG => "image/png",
                                    rig::message::ImageMediaType::GIF => "image/gif",
                                    rig::message::ImageMediaType::WEBP => "image/webp",
                                    rig::message::ImageMediaType::HEIC => "image/heic",
                                    rig::message::ImageMediaType::HEIF => "image/heif",
                                    rig::message::ImageMediaType::SVG => "image/svg+xml",
                                });
                                if let Some(part) = part_image(&data, media_type) {
                                    parts.push(part);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                append_turn(&mut contents, "user", parts);
            }
            Message::Assistant { content, .. } => {
                let mut parts = Vec::new();
                for block in content {
                    match block {
                        AssistantContent::Text(text) => {
                            if !text.text.trim().is_empty() {
                                parts.push(part_text(text.text.clone()));
                            }
                        }
                        AssistantContent::Reasoning(reasoning) => {
                            for block in &reasoning.content {
                                if let rig::message::ReasoningContent::Text { text, signature } = block
                                    && !text.trim().is_empty()
                                {
                                    parts.push(Part {
                                        text: Some(text.clone()),
                                        thought: Some(true),
                                        thought_signature: signature.clone(),
                                        ..Part::default()
                                    });
                                }
                            }
                        }
                        AssistantContent::ToolCall(call) => {
                            let raw_id = call.id.to_string();
                            let args_text = call.function.arguments.to_string();
                            let signed = call.signature.is_some();
                            if requires_sig && !signed {
                                dropped.insert(raw_id.clone(), args_text.clone());
                                dropped.insert(sanitize_tool_call_id(&raw_id), args_text);
                                continue;
                            }
                            parts.push(Part {
                                function_call: Some(FunctionCall {
                                    name: call.function.name.clone(),
                                    args: call.function.arguments.clone(),
                                    id: call_ids.then(|| sanitize_tool_call_id(&raw_id)),
                                }),
                                thought_signature: call.signature.clone(),
                                ..Part::default()
                            });
                        }
                        _ => {}
                    }
                }
                append_turn(&mut contents, "model", parts);
            }
        }
    }

    // The backend requires the first turn to be from the user.
    if contents.first().is_some_and(|first| first.role == "model") {
        contents.insert(
            0,
            Content {
                role: "user".to_string(),
                parts: vec![part_text("Hello")],
            },
        );
    }
    contents
}

fn image_data(data: &rig::message::DocumentSourceKind) -> Option<String> {
    match data {
        rig::message::DocumentSourceKind::Base64(data) => Some(data.clone()),
        rig::message::DocumentSourceKind::Raw(bytes) => {
            use base64::Engine;
            Some(base64::engine::general_purpose::STANDARD.encode(bytes))
        }
        rig::message::DocumentSourceKind::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn convert_tools(request: &CompletionRequest, legacy_parameters: bool) -> Option<Value> {
    if request.tools.is_empty() {
        return None;
    }
    let declarations: Vec<Value> = request
        .tools
        .iter()
        .map(|tool: &ToolDefinition| {
            let schema = strip_meta_schema(&tool.parameters);
            let schema = ensure_root_object(&schema);
            let mut declaration = json!({
                "name": tool.name,
                "description": tool.description,
            });
            if legacy_parameters {
                declaration["parameters"] = normalize_custom_tool_schema(&schema);
            } else {
                declaration["parametersJsonSchema"] = schema;
            }
            declaration
        })
        .collect();
    Some(json!([{ "functionDeclarations": declarations }]))
}

fn ensure_root_object(schema: &Value) -> Value {
    match schema {
        Value::Object(map) if map.contains_key("type") => schema.clone(),
        Value::Object(map) => {
            let mut out = map.clone();
            out.insert("type".to_string(), json!("object"));
            out.entry("properties").or_insert_with(|| json!({}));
            Value::Object(out)
        }
        _ => json!({ "type": "object", "properties": {} }),
    }
}

fn tool_config_mode(choice: Option<ToolChoice>) -> &'static str {
    match choice {
        Some(ToolChoice::None) => "NONE",
        Some(ToolChoice::Required | ToolChoice::Specific { .. }) => "ANY",
        _ => "VALIDATED",
    }
}

/// Model enum labels the backend expects for rollout-era runtime ids (pi parity).
fn model_enum_label(runtime: &str) -> Option<&'static str> {
    match runtime {
        "gemini-3.5-flash-extra-low" => Some("MODEL_PLACEHOLDER_M187"),
        "gemini-3.5-flash-low" => Some("MODEL_PLACEHOLDER_M20"),
        "gemini-3-flash-agent" => Some("MODEL_PLACEHOLDER_M132"),
        "gemini-3.1-pro-low" => Some("MODEL_PLACEHOLDER_M36"),
        "gemini-pro-agent" => Some("MODEL_PLACEHOLDER_M16"),
        _ => None,
    }
}

pub struct Envelope {
    pub request_id: String,
    pub session_id: String,
}

pub(crate) fn new_envelope() -> Envelope {
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

/// True when the runtime family wants the Claude interleaved-thinking beta
/// header enabled for the effort.
pub fn wants_claude_thinking_header(runtime_model: &str, effort: Effort) -> bool {
    effort != Effort::Off && runtime_model.starts_with("claude-")
}

/// Gemini thinkingConfig for the effort (pi parity). `Null` = omit the field
/// (Claude/GPT-OSS take the Claude beta header path instead).
fn thinking_config(runtime_model: &str, effort: Effort) -> Value {
    if !runtime_model.starts_with("gemini-") {
        return Value::Null;
    }
    if runtime_model.starts_with("gemini-3.5-flash") {
        return match effort {
            Effort::Off => json!({ "includeThoughts": false, "thinkingBudget": 0 }),
            Effort::Minimal | Effort::Low => {
                json!({ "includeThoughts": true, "thinkingBudget": 1000 })
            }
            Effort::Medium => json!({ "includeThoughts": true, "thinkingBudget": 4000 }),
            Effort::High => json!({ "includeThoughts": true, "thinkingBudget": 10000 }),
        };
    }
    if runtime_model.starts_with("gemini-3.1-pro") || runtime_model == "gemini-pro-agent" {
        return match effort {
            Effort::Off => json!({ "includeThoughts": false, "thinkingBudget": 0 }),
            Effort::High => json!({ "includeThoughts": true, "thinkingBudget": 10001 }),
            _ => json!({ "includeThoughts": true, "thinkingBudget": 1001 }),
        };
    }
    match effort {
        Effort::Off => json!({ "includeThoughts": false }),
        Effort::Minimal | Effort::Low => json!({ "includeThoughts": true, "thinkingLevel": "LOW" }),
        Effort::Medium => json!({ "includeThoughts": true, "thinkingLevel": "MEDIUM" }),
        Effort::High => json!({ "includeThoughts": true, "thinkingLevel": "HIGH" }),
    }
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
