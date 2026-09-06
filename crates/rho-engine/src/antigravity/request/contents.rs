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
pub struct FunctionResponsePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponse {
    pub name: String,
    pub response: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<FunctionResponsePart>>,
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

pub fn image_mime_type(media_type: Option<&rig::message::ImageMediaType>) -> &'static str {
    match media_type {
        Some(rig::message::ImageMediaType::JPEG) => "image/jpeg",
        Some(rig::message::ImageMediaType::PNG) => "image/png",
        Some(rig::message::ImageMediaType::GIF) => "image/gif",
        Some(rig::message::ImageMediaType::WEBP) => "image/webp",
        Some(rig::message::ImageMediaType::HEIC) => "image/heic",
        Some(rig::message::ImageMediaType::HEIF) => "image/heif",
        Some(rig::message::ImageMediaType::SVG) => "image/svg+xml",
        None => "image/png",
    }
}

pub fn tool_result_inline_data(image: &rig::message::Image) -> Option<InlineData> {
    let data = image_data(&image.data)?;
    let (mime, data) = match data.strip_prefix("data:") {
        Some(rest) => match rest.split_once(";base64,") {
            Some((m, d)) => (m.to_string(), d.to_string()),
            None => return None,
        },
        None => (image_mime_type(image.media_type.as_ref()).to_string(), data),
    };
    if data.is_empty() {
        return None;
    }
    Some(InlineData { mime_type: mime, data })
}

struct ConversionContext<'a> {
    dropped: &'a mut std::collections::HashMap<String, String>,
    requires_sig: bool,
    call_ids: bool,
}

fn extract_image_parts(content: &[rig::message::ToolResultContent]) -> Vec<FunctionResponsePart> {
    content
        .iter()
        .filter_map(|c| match c {
            rig::message::ToolResultContent::Image(image) => {
                tool_result_inline_data(image).map(|inline_data| FunctionResponsePart {
                    inline_data: Some(inline_data),
                })
            }
            _ => None,
        })
        .collect()
}

fn push_observation_parts(
    (result_name, args, response_text): (&str, &str, &str),
    image_parts: Vec<FunctionResponsePart>,
    parts: &mut Vec<Part>,
) {
    let label = if args == "{}" {
        format!("`{result_name}`")
    } else {
        format!("`{result_name}` ({args})")
    };
    parts.push(part_text(format!("[Observation from {label}:\n{response_text}]")));
    for p in image_parts {
        if let Some(inline_data) = p.inline_data {
            parts.push(Part {
                inline_data: Some(inline_data),
                ..Part::default()
            });
        }
    }
}

fn build_function_response_part(
    result: &rig::message::ToolResult,
    (sanitized_id, response_text, image_parts): (String, String, Vec<FunctionResponsePart>),
    ctx: &ConversionContext<'_>,
) -> Part {
    Part {
        function_response: Some(FunctionResponse {
            name: result.name.clone(),
            response: json!({ "output": response_text }),
            id: ctx.call_ids.then_some(sanitized_id),
            parts: (!image_parts.is_empty()).then_some(image_parts),
        }),
        ..Part::default()
    }
}

fn convert_tool_result_part(result: &rig::message::ToolResult, ctx: &ConversionContext<'_>, parts: &mut Vec<Part>) {
    let response_text = tool_result_text(&result.content);
    let raw_id = result.call.to_string();
    let sanitized_id = sanitize_tool_call_id(&raw_id);
    let image_parts = extract_image_parts(&result.content);

    let dropped_args = ctx
        .requires_sig
        .then(|| {
            ctx.dropped
                .get(&raw_id)
                .or_else(|| ctx.dropped.get(&sanitized_id))
                .cloned()
        })
        .flatten();
    if let Some(args) = dropped_args {
        push_observation_parts((&result.name, &args, &response_text), image_parts, parts);
    } else {
        let part = build_function_response_part(result, (sanitized_id, response_text, image_parts), ctx);
        parts.push(part);
    }
}

fn convert_image_part(image: &rig::message::Image) -> Option<Part> {
    let data = image_data(&image.data)?;
    let media_type = image_mime_type(image.media_type.as_ref());
    part_image(&data, Some(media_type))
}

fn convert_user_content(content: &[UserContent], ctx: &ConversionContext<'_>) -> Vec<Part> {
    let mut parts = Vec::new();
    for item in content {
        match item {
            UserContent::Text(text) if !text.text.trim().is_empty() => parts.push(part_text(text.text.clone())),
            UserContent::ToolResult(result) => convert_tool_result_part(result, ctx, &mut parts),
            UserContent::Image(image) => {
                if let Some(part) = convert_image_part(image) {
                    parts.push(part);
                }
            }
            _ => {}
        }
    }
    parts
}

fn convert_reasoning_part(reasoning: &rig::message::Reasoning, parts: &mut Vec<Part>) {
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

fn convert_tool_call_part(call: &rig::message::ToolCall, ctx: &mut ConversionContext<'_>, parts: &mut Vec<Part>) {
    let raw_id = call.id.to_string();
    let args_text = call.function.arguments.to_string();
    if ctx.requires_sig && call.signature.is_none() {
        ctx.dropped.insert(raw_id.clone(), args_text.clone());
        ctx.dropped.insert(sanitize_tool_call_id(&raw_id), args_text);
        return;
    }
    parts.push(Part {
        function_call: Some(FunctionCall {
            name: call.function.name.clone(),
            args: call.function.arguments.clone(),
            id: ctx.call_ids.then(|| sanitize_tool_call_id(&raw_id)),
        }),
        thought_signature: call.signature.clone(),
        ..Part::default()
    });
}

fn convert_assistant_content(content: &[AssistantContent], ctx: &mut ConversionContext<'_>) -> Vec<Part> {
    let mut parts = Vec::new();
    for block in content {
        match block {
            AssistantContent::Text(text) if !text.text.trim().is_empty() => parts.push(part_text(text.text.clone())),
            AssistantContent::Reasoning(reasoning) => convert_reasoning_part(reasoning, &mut parts),
            AssistantContent::ToolCall(call) => convert_tool_call_part(call, ctx, &mut parts),
            _ => {}
        }
    }
    parts
}

fn convert_turn_content(message: &Message, ctx: &mut ConversionContext<'_>, contents: &mut Vec<Content>) {
    match message {
        Message::System { .. } => {}
        Message::User { content } => {
            let parts = convert_user_content(content, ctx);
            append_turn(contents, "user", parts);
        }
        Message::Assistant { content, .. } => {
            let parts = convert_assistant_content(content, ctx);
            append_turn(contents, "model", parts);
        }
    }
}

pub fn convert_contents(request: &CompletionRequest, runtime_model: &str) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();
    let mut dropped = std::collections::HashMap::new();
    let requires_sig = gemini_requires_thought_signature(runtime_model);
    let call_ids = needs_function_call_id(runtime_model);
    let mut ctx = ConversionContext {
        dropped: &mut dropped,
        requires_sig,
        call_ids,
    };

    for message in &request.chat_history {
        convert_turn_content(message, &mut ctx, &mut contents);
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
