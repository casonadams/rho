use super::model::{gemini_requires_thought_signature, needs_function_call_id, sanitize_tool_call_id};
use crate::provider::adapter::ModelCompletionRequest;
use rho_harness_core::model::{AssistantContent, ChatMessage, ToolResultContent, UserContent};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<Vec<FunctionResponsePart>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FunctionResponsePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub role: String,
    pub parts: Vec<Part>,
}

pub const SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

pub fn part_text(text: impl Into<String>) -> Part {
    Part {
        text: Some(text.into()),
        ..Part::default()
    }
}

pub struct ConversionContext<'a> {
    pub requires_sig: bool,
    pub call_ids: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

fn convert_tool_result_part(result: &rho_harness_core::model::ToolResult, call_ids: bool) -> Part {
    let mut text = String::new();
    let mut image_parts = Vec::new();
    for c in &result.content {
        match c {
            ToolResultContent::Text(t) => text.push_str(&t.text),
            ToolResultContent::Image(img) => {
                image_parts.push(FunctionResponsePart {
                    inline_data: Some(InlineData {
                        mime_type: img.media_type.clone().unwrap_or_else(|| "image/png".to_string()),
                        data: img.data.clone(),
                    }),
                });
            }
            ToolResultContent::Json { value } => text.push_str(&value.to_string()),
        }
    }
    Part {
        function_response: Some(FunctionResponse {
            name: result.name.clone(),
            response: json!({ "output": text }),
            id: call_ids.then(|| sanitize_tool_call_id(&result.call)),
            parts: (!image_parts.is_empty()).then_some(image_parts),
        }),
        ..Part::default()
    }
}

fn convert_antigravity_user_part(part: &UserContent, call_ids: bool) -> Option<Part> {
    match part {
        UserContent::Text(t) => {
            let trimmed = t.text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(part_text(&t.text))
            }
        }
        UserContent::Image(img) => Some(Part {
            inline_data: Some(InlineData {
                mime_type: img.media_type.clone().unwrap_or_else(|| "image/png".to_string()),
                data: img.data.clone(),
            }),
            ..Part::default()
        }),
        UserContent::ToolResult(result) => Some(convert_tool_result_part(result, call_ids)),
    }
}

fn convert_antigravity_user(content: &[UserContent], call_ids: bool) -> Vec<Part> {
    content
        .iter()
        .filter_map(|p| convert_antigravity_user_part(p, call_ids))
        .collect()
}

fn convert_antigravity_assistant_part(
    part: &AssistantContent,
    ctx: &ConversionContext<'_>,
    is_first_call: &mut bool,
) -> Option<Part> {
    match part {
        AssistantContent::Reasoning(reasoning) => {
            let (text_opt, sig_opt) = reasoning.first_text_and_signature();
            let text = text_opt?.trim();
            if text.is_empty() {
                None
            } else {
                Some(Part {
                    text: Some(text.to_string()),
                    thought: Some(true),
                    thought_signature: sig_opt.map(str::to_string),
                    ..Part::default()
                })
            }
        }
        AssistantContent::Text(t) => {
            let trimmed = t.text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(part_text(&t.text))
            }
        }
        AssistantContent::ToolCall(call) => {
            let sig = call.signature.clone().or_else(|| {
                if ctx.requires_sig && *is_first_call {
                    Some(SKIP_THOUGHT_SIGNATURE.to_string())
                } else {
                    None
                }
            });
            *is_first_call = false;
            Some(Part {
                function_call: Some(FunctionCall {
                    name: call.function.name.clone(),
                    args: call.function.arguments.clone(),
                    id: ctx.call_ids.then(|| sanitize_tool_call_id(&call.id)),
                }),
                thought_signature: sig,
                ..Part::default()
            })
        }
        _ => None,
    }
}

fn convert_antigravity_assistant(content: &[AssistantContent], ctx: &ConversionContext<'_>) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut is_first = true;
    for part in content {
        if let Some(p) = convert_antigravity_assistant_part(part, ctx, &mut is_first) {
            parts.push(p);
        }
    }
    parts
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

pub fn convert_contents(request: &ModelCompletionRequest, runtime_model: &str) -> Vec<Content> {
    let mut contents: Vec<Content> = Vec::new();
    let requires_sig = gemini_requires_thought_signature(runtime_model);
    let call_ids = needs_function_call_id(runtime_model);
    let ctx = ConversionContext {
        requires_sig,
        call_ids,
        _marker: std::marker::PhantomData,
    };

    for message in &request.messages {
        match message {
            ChatMessage::System { .. } => {}
            ChatMessage::User { content } => {
                let parts = convert_antigravity_user(content, ctx.call_ids);
                append_turn(&mut contents, "user", parts);
            }
            ChatMessage::Assistant { content, .. } => {
                let parts = convert_antigravity_assistant(content, &ctx);
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
