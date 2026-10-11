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
    let (mime, data) = if let Some(rest) = data.strip_prefix("data:") {
        rest.split_once(";base64,")?
    } else {
        (image_mime_type(image.media_type.as_ref()), data.as_str())
    };
    if data.is_empty() {
        return None;
    }
    Some(InlineData {
        mime_type: mime.to_string(),
        data: data.to_string(),
    })
}

struct ConversionContext<'a> {
    requires_sig: bool,
    call_ids: bool,
    _marker: std::marker::PhantomData<&'a ()>,
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

fn build_function_response_part(
    result: &rig::message::ToolResult,
    sanitized_id: String,
    response_text: String,
    image_parts: Vec<FunctionResponsePart>,
    ctx: &ConversionContext<'_>,
) -> Part {
    Part {
        function_response: Some(FunctionResponse {
            name: result.name.to_string(),
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
    let part = build_function_response_part(result, sanitized_id, response_text, image_parts, ctx);
    parts.push(part);
}

fn convert_image_part(image: &rig::message::Image) -> Option<Part> {
    tool_result_inline_data(image).map(|inline_data| Part {
        inline_data: Some(inline_data),
        ..Part::default()
    })
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
    if !reasoning.text.trim().is_empty() {
        let signature = reasoning
            .native
            .as_ref()
            .and_then(|n| n.item.get("signature"))
            .and_then(|s| s.as_str())
            .map(ToString::to_string);
        parts.push(Part {
            text: Some(reasoning.text.clone()),
            thought: Some(true),
            thought_signature: signature,
            ..Part::default()
        });
    }
}

const SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

fn convert_tool_call_part(
    call: &rig::message::ToolCall,
    ctx: &ConversionContext<'_>,
    is_first_tool_call: &mut bool,
    parts: &mut Vec<Part>,
) {
    let raw_id = call.id.to_string();
    let args_val = serde_json::Value::Object(call.function.arguments.clone());
    let signature = call
        .native
        .as_ref()
        .and_then(|n| n.item.get("signature"))
        .and_then(|s| s.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            if ctx.requires_sig && *is_first_tool_call {
                Some(SKIP_THOUGHT_SIGNATURE.to_string())
            } else {
                None
            }
        });
    *is_first_tool_call = false;

    parts.push(Part {
        function_call: Some(FunctionCall {
            name: call.function.name.to_string(),
            args: args_val,
            id: ctx.call_ids.then(|| sanitize_tool_call_id(&raw_id)),
        }),
        thought_signature: signature,
        ..Part::default()
    });
}

fn convert_assistant_content(content: &[AssistantContent], ctx: &ConversionContext<'_>) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut is_first_tool_call = true;
    for block in content {
        match block {
            AssistantContent::Text(text) if !text.text.trim().is_empty() => parts.push(part_text(text.text.clone())),
            AssistantContent::Reasoning(reasoning) => {
                convert_reasoning_part(reasoning, &mut parts);
            }
            AssistantContent::ToolCall(call) => {
                convert_tool_call_part(call, ctx, &mut is_first_tool_call, &mut parts);
            }
            _ => {}
        }
    }
    parts
}

fn convert_turn_content(message: &Message, ctx: &ConversionContext<'_>, contents: &mut Vec<Content>) {
    match message {
        Message::System { .. } => {}
        Message::User { content } => {
            let parts = convert_user_content(content, ctx);
            append_turn(contents, "user", parts);
        }
        Message::Assistant(asst) => {
            let parts = convert_assistant_content(&asst.content, ctx);
            append_turn(contents, "model", parts);
        }
    }
}

pub fn convert_contents(request: &CompletionRequest, runtime_model: &str) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();
    let requires_sig = gemini_requires_thought_signature(runtime_model);
    let call_ids = needs_function_call_id(runtime_model);
    let ctx = ConversionContext {
        requires_sig,
        call_ids,
        _marker: std::marker::PhantomData,
    };

    for message in &request.chat_history {
        convert_turn_content(message, &ctx, &mut contents);
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
