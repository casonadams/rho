use crate::engine::metrics::StructuralUsage;
use crate::engine::provider::host_loop::ProviderUsage;
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage};

pub(crate) struct ExternalTurnArtifacts {
    pub(crate) output: crate::engine::provider::host_loop::NeutralTurnOutput,
    pub(crate) tool_calls_count: usize,
    pub(crate) completed_tools: Vec<super::super::sink::CompletedTool>,
}

pub(crate) fn rig_history_to_neutral(history: &[rig::message::Message]) -> Vec<ModelMessage> {
    let mut messages = Vec::new();
    for message in history {
        match message {
            rig::message::Message::System { .. } => {}
            rig::message::Message::User { content } => {
                for item in content {
                    match item {
                        rig::message::UserContent::Text(text) => messages.push(ModelMessage {
                            role: MessageRole::User,
                            content: vec![MessageContent::Text {
                                text: text.text.clone(),
                            }],
                        }),
                        rig::message::UserContent::ToolResult(result) => {
                            let content = result
                                .content
                                .iter()
                                .filter_map(|item| item.as_text().map(str::to_string))
                                .collect::<Vec<_>>()
                                .join("\n");
                            messages.push(ModelMessage {
                                role: MessageRole::Tool,
                                content: vec![MessageContent::ToolResult {
                                    call_id: result.call.as_str().to_string(),
                                    content,
                                    is_error: false,
                                }],
                            });
                        }
                        _ => {}
                    }
                }
            }
            rig::message::Message::Assistant { content, .. } => {
                for item in content {
                    match item {
                        rig::message::AssistantContent::Text(text) => messages.push(ModelMessage {
                            role: MessageRole::Assistant,
                            content: vec![MessageContent::Text {
                                text: text.text.clone(),
                            }],
                        }),
                        rig::message::AssistantContent::ToolCall(call) => messages.push(ModelMessage {
                            role: MessageRole::Assistant,
                            content: vec![MessageContent::ToolCall {
                                call_id: call.id.as_str().to_string(),
                                tool_id: format!("tool:{}", call.function.name)
                                    .parse()
                                    .unwrap_or_else(|_| "tool:unknown".parse().unwrap()),
                                arguments: call.function.arguments.clone(),
                            }],
                        }),
                        _ => {}
                    }
                }
            }
        }
    }
    messages
}

pub(crate) fn neutral_history_to_rig(messages: &[ModelMessage]) -> Vec<rig::message::Message> {
    let tool_names: std::collections::HashMap<&str, &str> = messages
        .iter()
        .filter_map(|message| match message {
            ModelMessage {
                role: MessageRole::Assistant,
                content,
            } => content.iter().find_map(|item| match item {
                MessageContent::ToolCall { call_id, tool_id, .. } => Some((call_id.as_str(), tool_id.name())),
                _ => None,
            }),
            _ => None,
        })
        .collect();
    let mut canonical = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        match &messages[index] {
            ModelMessage {
                role: MessageRole::System,
                ..
            } => index += 1,
            ModelMessage {
                role: MessageRole::User,
                content,
            } => {
                for item in content {
                    if let MessageContent::Text { text } = item {
                        canonical.push(rig::message::Message::user(text.clone()));
                    }
                }
                index += 1;
            }
            ModelMessage {
                role: MessageRole::Assistant,
                content,
            } => {
                let mut assistant_content = Vec::new();
                for item in content {
                    match item {
                        MessageContent::Text { text } => {
                            assistant_content.push(rig::message::AssistantContent::text(text.clone()));
                        }
                        MessageContent::ToolCall {
                            call_id,
                            tool_id,
                            arguments,
                        } => assistant_content.push(rig::message::AssistantContent::ToolCall(
                            rig::message::ToolCall::new(
                                rig::message::ToolCallId::new_or_mint(call_id.clone()),
                                rig::message::ToolFunction::new(tool_id.name().to_string(), arguments.clone()),
                            ),
                        )),
                        MessageContent::ToolResult { .. } => {}
                    }
                }
                if !assistant_content.is_empty() {
                    canonical.push(rig::message::Message::Assistant {
                        id: None,
                        content: assistant_content,
                    });
                }
                index += 1;
            }
            ModelMessage {
                role: MessageRole::Tool,
                ..
            } => {
                let mut results = Vec::new();
                while let Some(ModelMessage {
                    role: MessageRole::Tool,
                    content,
                }) = messages.get(index)
                {
                    for item in content {
                        if let MessageContent::ToolResult { call_id, content, .. } = item {
                            results.push(rig::message::UserContent::ToolResult(rig::message::ToolResult {
                                call: rig::message::ToolCallId::new_or_mint(call_id.clone()),
                                provider: None,
                                name: tool_names.get(call_id.as_str()).unwrap_or(&"unknown").to_string(),
                                content: vec![rig::message::ToolResultContent::Text(rig::message::Text::new(
                                    content.clone(),
                                ))],
                            }));
                        }
                    }
                    index += 1;
                }
                if !results.is_empty() {
                    canonical.push(rig::message::Message::User { content: results });
                }
            }
        }
    }
    canonical
}

pub(crate) fn structural_usage(usage: ProviderUsage) -> StructuralUsage {
    let active_input = if usage.last_input_tokens > 0 {
        usage.last_input_tokens
    } else {
        usage.input_tokens
    };
    StructuralUsage {
        input_tokens: active_input,
        output_tokens: usage.output_tokens,
        total_tokens: active_input.saturating_add(usage.output_tokens),
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        tool_use_prompt_tokens: None,
        reasoning_tokens: None,
    }
}
