use super::model::{gemini_requires_thought_signature, needs_function_call_id, sanitize_tool_call_id};
use rig::completion::CompletionRequest;
use rig::message::{AssistantContent, Message, UserContent};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InlineData {
    pub mime_type: String,
    pub data: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionCall {
    pub name: String,
    pub args: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    pub name: String,
    pub response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub role: String,
    pub parts: Vec<Part>,
}

pub fn part_text(text: impl Into<String>) -> Part {
    Part {
        text: Some(text.into()),
        ..Part::default()
    }
}

pub fn part_image(data: &str, media_type: Option<&str>) -> Option<Part> {
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

pub fn append_turn(contents: &mut Vec<Content>, role: &str, parts: Vec<Part>) {
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

pub fn tool_result_text(content: &[rig::message::ToolResultContent]) -> String {
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

pub fn image_data(data: &rig::message::DocumentSourceKind) -> Option<String> {
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

/// Convert rig chat history into Gemini `contents`, mirroring pi's replay
/// rules: unsigned tool calls on Gemini 3+ are dropped and their results are
/// replayed as user observations, because the backend validates thought
/// signatures on Gemini 3 function-call replay.
pub fn convert_contents(request: &CompletionRequest, runtime_model: &str) -> Vec<Content> {
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
