//! Native Google Generative Language API (`v1beta`) request builder.
//!
//! Targets `generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse`
//! with first-class Gemini 4 / Gemini 3 support: `thinkingLevel` vs `thinkingBudget`,
//! `includeThoughts`, per-call `thoughtSignature` validation & replay, function call IDs,
//! multimodal `functionResponse.parts`, and `parametersJsonSchema` tool declarations.

use crate::antigravity::request::contents::{
    Content, FunctionCall, FunctionResponse, FunctionResponsePart, InlineData, Part, SKIP_THOUGHT_SIGNATURE, part_text,
};
use crate::antigravity::request::model::sanitize_tool_call_id;
use crate::antigravity::request::schema::convert_tools;
use crate::provider::adapter::ModelCompletionRequest;
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolResult, ToolResultContent, UserContent};
use rho_harness_core::thinking::ThinkingLevel;
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ThinkingFamily {
    /// Gemini 4 and Gemini 3.7+: `thinkingLevel` in `LOW | MEDIUM | HIGH` (API rejects `MINIMAL`).
    LevelNoMinimal,
    /// Gemini 3 / 3.1 Pro: `thinkingLevel` in `LOW | HIGH`.
    LevelPro,
    /// Gemini 3 / 3.5 / 3.6 Flash: `thinkingLevel` in `MINIMAL | LOW | MEDIUM | HIGH`.
    LevelWithMinimal,
    /// Gemini 2.5: integer `thinkingBudget`.
    Budget,
    /// Non-thinking models (e.g. `gemini-decision-*`, `gemini-2.0-*`, `gemini-1.5-*`).
    None,
}

fn thinking_family(model: &str) -> ThinkingFamily {
    let m = model.to_ascii_lowercase();
    if m.starts_with("gemini-4") {
        return ThinkingFamily::LevelNoMinimal;
    }
    if m.starts_with("gemini-3.7") || m.starts_with("gemini-3.8") || m.starts_with("gemini-3.9") {
        return if m.contains("pro") {
            ThinkingFamily::LevelPro
        } else {
            ThinkingFamily::LevelNoMinimal
        };
    }
    if m.starts_with("gemini-3") {
        return if m.contains("pro") {
            ThinkingFamily::LevelPro
        } else {
            ThinkingFamily::LevelWithMinimal
        };
    }
    if m.starts_with("gemini-2.5") {
        return ThinkingFamily::Budget;
    }
    ThinkingFamily::None
}

/// True for Gemini 3+ and Gemini 4+ models on the direct Google Gemini API.
pub fn is_gemini_3_or_4(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("gemini-3") || m.starts_with("gemini-4")
}

/// Validates a replayed thought signature so signatures from other providers (Claude, OpenAI)
/// are never forwarded to Gemini.
pub fn is_valid_thought_signature(sig: &str) -> bool {
    if sig == SKIP_THOUGHT_SIGNATURE {
        return true;
    }
    !sig.is_empty()
        && sig.len().is_multiple_of(4)
        && sig
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
}

fn level_thinking_config(level: &str, include_thoughts: bool) -> Value {
    json!({
        "includeThoughts": include_thoughts,
        "thinkingLevel": level,
    })
}

fn budget_thinking_config(effort: ThinkingLevel) -> Value {
    match effort {
        ThinkingLevel::Off => json!({ "includeThoughts": false, "thinkingBudget": 0 }),
        ThinkingLevel::Minimal => json!({ "includeThoughts": true, "thinkingBudget": 1_024 }),
        ThinkingLevel::Low => json!({ "includeThoughts": true, "thinkingBudget": 2_048 }),
        ThinkingLevel::Medium => json!({ "includeThoughts": true, "thinkingBudget": 8_192 }),
        ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => {
            json!({ "includeThoughts": true, "thinkingBudget": -1 })
        }
    }
}

/// Builds `generationConfig.thinkingConfig` for direct Google Generative Language API calls.
/// Returns `Value::Null` for non-thinking models so `thinkingConfig` is omitted.
pub fn thinking_config(model: &str, thinking_level: Option<&str>) -> Value {
    let effort = thinking_level
        .and_then(ThinkingLevel::parse)
        .unwrap_or(ThinkingLevel::Medium);
    let include = effort != ThinkingLevel::Off;

    match thinking_family(model) {
        ThinkingFamily::None => Value::Null,
        ThinkingFamily::Budget => budget_thinking_config(effort),
        ThinkingFamily::LevelNoMinimal => {
            let level = match effort {
                ThinkingLevel::Off | ThinkingLevel::Minimal | ThinkingLevel::Low => "LOW",
                ThinkingLevel::Medium => "MEDIUM",
                ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => "HIGH",
            };
            level_thinking_config(level, include)
        }
        ThinkingFamily::LevelPro => {
            let level = match effort {
                ThinkingLevel::Off | ThinkingLevel::Minimal | ThinkingLevel::Low => "LOW",
                ThinkingLevel::Medium | ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => "HIGH",
            };
            level_thinking_config(level, include)
        }
        ThinkingFamily::LevelWithMinimal => {
            let level = match effort {
                ThinkingLevel::Off | ThinkingLevel::Minimal => "MINIMAL",
                ThinkingLevel::Low => "LOW",
                ThinkingLevel::Medium => "MEDIUM",
                ThinkingLevel::High | ThinkingLevel::XHigh | ThinkingLevel::Max => "HIGH",
            };
            level_thinking_config(level, include)
        }
    }
}

fn build_generation_config(
    model: &str,
    thinking_level: Option<&str>,
    request: &ModelCompletionRequest,
) -> Option<Value> {
    let mut cfg = serde_json::Map::new();
    if let Some(temp) = request.temperature {
        cfg.insert("temperature".to_string(), json!(temp));
    }
    if let Some(max) = request.max_tokens {
        cfg.insert("maxOutputTokens".to_string(), json!(max));
    }
    let thinking = thinking_config(model, thinking_level);
    if !thinking.is_null() {
        cfg.insert("thinkingConfig".to_string(), thinking);
    }
    (!cfg.is_empty()).then_some(Value::Object(cfg))
}

#[derive(Clone, Copy)]
struct ReplayCaps {
    /// Gemini 3+ direct API requires a signature (or `SKIP_THOUGHT_SIGNATURE`) on every function call.
    requires_sig: bool,
    /// Gemini 3+ direct API supports `functionCall.id` / `functionResponse.id`.
    call_ids: bool,
    /// Gemini 3+ supports nested `functionResponse.parts` for multimodal tool outputs.
    multimodal_tool_response: bool,
}

impl ReplayCaps {
    fn for_model(model: &str) -> Self {
        let modern = is_gemini_3_or_4(model);
        Self {
            requires_sig: modern,
            call_ids: modern,
            multimodal_tool_response: modern,
        }
    }
}

fn convert_tool_result_parts(result: &ToolResult, caps: ReplayCaps) -> Vec<Part> {
    let mut text = String::new();
    let mut inline_images = Vec::new();
    for item in &result.content {
        match item {
            ToolResultContent::Text(t) => text.push_str(&t.text),
            ToolResultContent::Image(img) => inline_images.push(InlineData {
                mime_type: img.media_type_or_png().to_string(),
                data: img.data.clone(),
            }),
            ToolResultContent::Json { value } => text.push_str(&value.to_string()),
        }
    }

    let nested_parts = if caps.multimodal_tool_response && !inline_images.is_empty() {
        Some(
            inline_images
                .drain(..)
                .map(|inline_data| FunctionResponsePart {
                    inline_data: Some(inline_data),
                })
                .collect(),
        )
    } else {
        None
    };

    let mut out = vec![Part {
        function_response: Some(FunctionResponse {
            name: result.name.clone(),
            response: json!({ "output": text }),
            id: caps.call_ids.then(|| sanitize_tool_call_id(&result.call)),
            parts: nested_parts,
        }),
        ..Part::default()
    }];

    for inline_data in inline_images {
        out.push(Part {
            inline_data: Some(inline_data),
            ..Part::default()
        });
    }
    out
}

fn convert_user_content(content: &[UserContent], caps: ReplayCaps) -> Vec<Part> {
    let mut parts = Vec::new();
    for item in content {
        match item {
            UserContent::Text(t) if !t.text.trim().is_empty() => parts.push(part_text(&t.text)),
            UserContent::Text(_) => {}
            UserContent::Image(img) => parts.push(Part {
                inline_data: Some(InlineData {
                    mime_type: img.media_type_or_png().to_string(),
                    data: img.data.clone(),
                }),
                ..Part::default()
            }),
            UserContent::ToolResult(result) => parts.extend(convert_tool_result_parts(result, caps)),
        }
    }
    parts
}

fn validated_signature(sig: Option<&str>) -> Option<String> {
    sig.filter(|s| is_valid_thought_signature(s)).map(str::to_string)
}

fn convert_assistant_part(part: &AssistantContent, caps: ReplayCaps) -> Option<Part> {
    match part {
        AssistantContent::Reasoning(reasoning) => {
            let (text_opt, sig_opt) = reasoning.first_text_and_signature();
            let text = text_opt?.trim();
            let sig = validated_signature(sig_opt)?;
            if text.is_empty() {
                return None;
            }
            Some(Part {
                text: Some(text.to_string()),
                thought: Some(true),
                thought_signature: Some(sig),
                ..Part::default()
            })
        }
        AssistantContent::Text(t) if !t.text.trim().is_empty() => Some(part_text(&t.text)),
        AssistantContent::ToolCall(call) => {
            let sig = validated_signature(call.signature.as_deref())
                .or_else(|| caps.requires_sig.then(|| SKIP_THOUGHT_SIGNATURE.to_string()));
            Some(Part {
                function_call: Some(FunctionCall {
                    name: call.function.name.clone(),
                    args: call.function.arguments.clone(),
                    id: caps.call_ids.then(|| sanitize_tool_call_id(&call.id)),
                }),
                thought_signature: sig,
                ..Part::default()
            })
        }
        _ => None,
    }
}

fn append_turn(contents: &mut Vec<Content>, role: &str, mut parts: Vec<Part>) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = contents.last_mut()
        && last.role == role
    {
        last.parts.append(&mut parts);
    } else {
        contents.push(Content {
            role: role.to_string(),
            parts,
        });
    }
}

pub fn convert_contents(request: &ModelCompletionRequest, model: &str) -> Vec<Content> {
    let caps = ReplayCaps::for_model(model);
    let mut contents = Vec::new();
    for message in &request.messages {
        match message {
            ChatMessage::System { content } if !content.trim().is_empty() => {
                append_turn(&mut contents, "user", vec![part_text(content)]);
            }
            ChatMessage::System { .. } => {}
            ChatMessage::User { content } => {
                append_turn(&mut contents, "user", convert_user_content(content, caps));
            }
            ChatMessage::Assistant { content, .. } => {
                let parts = content.iter().filter_map(|p| convert_assistant_part(p, caps)).collect();
                append_turn(&mut contents, "model", parts);
            }
        }
    }
    if !request.prompt.trim().is_empty() {
        append_turn(&mut contents, "user", vec![part_text(request.prompt.trim())]);
    }
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

pub fn build_request_body(model: &str, thinking_level: Option<&str>, request: &ModelCompletionRequest) -> Value {
    let mut body = json!({
        "contents": convert_contents(request, model),
    });
    if let Some(preamble) = request.preamble.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        body["systemInstruction"] = json!({
            "parts": [{ "text": preamble }],
        });
    }
    if let Some(gen_cfg) = build_generation_config(model, thinking_level, request) {
        body["generationConfig"] = gen_cfg;
    }
    if let Some(tools) = convert_tools(request, false) {
        body["tools"] = tools;
        body["toolConfig"] = json!({
            "functionCallingConfig": { "mode": "AUTO" },
        });
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::adapter::ModelToolDefinition;
    use rho_harness_core::model::{Reasoning, ToolCall, ToolFunction, ToolResult};

    #[test]
    fn gemini_4_thinking_levels_avoid_minimal_and_enable_thoughts() {
        assert_eq!(
            thinking_config("gemini-4-neon", Some("high")),
            json!({ "includeThoughts": true, "thinkingLevel": "HIGH" })
        );
        assert_eq!(
            thinking_config("gemini-4-hydrogen", Some("low")),
            json!({ "includeThoughts": true, "thinkingLevel": "LOW" })
        );
        assert_eq!(
            thinking_config("gemini-4-hydrogen", Some("minimal")),
            json!({ "includeThoughts": true, "thinkingLevel": "LOW" })
        );
        assert_eq!(
            thinking_config("gemini-4-carbon", Some("medium")),
            json!({ "includeThoughts": true, "thinkingLevel": "MEDIUM" })
        );
        assert_eq!(
            thinking_config("gemini-4-neon", Some("off")),
            json!({ "includeThoughts": false, "thinkingLevel": "LOW" })
        );
    }

    #[test]
    fn non_thinking_and_legacy_gemini_models_use_appropriate_config() {
        assert_eq!(thinking_config("gemini-decision-1", Some("off")), Value::Null);
        assert_eq!(thinking_config("gemini-2.0-flash", Some("medium")), Value::Null);
        assert_eq!(
            thinking_config("gemini-2.5-pro", Some("high")),
            json!({ "includeThoughts": true, "thinkingBudget": -1 })
        );
        assert_eq!(
            thinking_config("gemini-3.5-flash", Some("minimal")),
            json!({ "includeThoughts": true, "thinkingLevel": "MINIMAL" })
        );
        assert_eq!(
            thinking_config("gemini-3.1-pro-preview", Some("medium")),
            json!({ "includeThoughts": true, "thinkingLevel": "HIGH" })
        );
    }

    #[test]
    fn gemini_4_replays_valid_signatures_and_falls_back_on_every_unsigned_call() {
        let mut call_signed = ToolCall::new("c1", ToolFunction::new("read", json!({"path": "a.rs"})));
        call_signed.signature = Some("YWJjZA==".to_string());
        let call_unsigned = ToolCall::new("c2", ToolFunction::new("read", json!({"path": "b.rs"})));
        let mut call_foreign = ToolCall::new("c3", ToolFunction::new("read", json!({"path": "c.rs"})));
        call_foreign.signature = Some("not valid base64!".to_string());

        let req = ModelCompletionRequest {
            preamble: Some("Be concise.".to_string()),
            messages: vec![
                ChatMessage::user("check files"),
                ChatMessage::Assistant {
                    id: None,
                    content: vec![
                        AssistantContent::Reasoning(Reasoning::new_with_signature(
                            "valid thought",
                            Some("YWJjZA==".to_string()),
                        )),
                        AssistantContent::Reasoning(Reasoning::new_with_signature("foreign thought", None)),
                        AssistantContent::ToolCall(call_signed),
                        AssistantContent::ToolCall(call_unsigned),
                        AssistantContent::ToolCall(call_foreign),
                    ],
                },
            ],
            tools: vec![ModelToolDefinition {
                name: "read".to_string(),
                description: "Read file".to_string(),
                parameters: json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }),
            }],
            ..Default::default()
        };

        let body = build_request_body("gemini-4-neon", Some("medium"), &req);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "Be concise.");
        assert!(body["systemInstruction"].get("role").is_none());
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "AUTO");
        assert!(
            body["tools"][0]["functionDeclarations"][0]
                .get("parametersJsonSchema")
                .is_some()
        );

        let parts = body["contents"][1]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 4); // 1 valid reasoning + 3 tool calls (foreign unsigned reasoning skipped)
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["thoughtSignature"], "YWJjZA==");
        assert_eq!(parts[1]["thoughtSignature"], "YWJjZA==");
        assert_eq!(parts[1]["functionCall"]["id"], "c1");
        assert_eq!(parts[2]["thoughtSignature"], SKIP_THOUGHT_SIGNATURE);
        assert_eq!(parts[2]["functionCall"]["id"], "c2");
        assert_eq!(parts[3]["thoughtSignature"], SKIP_THOUGHT_SIGNATURE);
    }

    #[test]
    fn multimodal_tool_result_nests_images_on_gemini_4_and_separates_on_gemini_2() {
        let result = ToolResult {
            call: "call-1".to_string(),
            name: "read".to_string(),
            provider: None,
            content: vec![
                ToolResultContent::text("ok"),
                ToolResultContent::image("aW1n", Some("image/png".to_string())),
            ],
        };
        let req = ModelCompletionRequest {
            messages: vec![ChatMessage::User {
                content: vec![UserContent::ToolResult(result)],
            }],
            ..Default::default()
        };

        let body4 = build_request_body("gemini-4-neon", Some("low"), &req);
        let p4 = body4["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(p4.len(), 1);
        assert_eq!(p4[0]["functionResponse"]["id"], "call-1");
        assert_eq!(
            p4[0]["functionResponse"]["parts"][0]["inlineData"]["mimeType"],
            "image/png"
        );

        let body2 = build_request_body("gemini-2.5-flash", Some("off"), &req);
        let p2 = body2["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(p2.len(), 2);
        assert!(p2[0]["functionResponse"].get("id").is_none());
        assert!(p2[0]["functionResponse"].get("parts").is_none());
        assert_eq!(p2[1]["inlineData"]["mimeType"], "image/png");
    }
}
