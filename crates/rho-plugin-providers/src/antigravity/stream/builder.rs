use crate::antigravity::types::*;
use rho_sdk::contract::{MessageContent, MessageRole, ProviderRequest};
use std::sync::LazyLock;

pub static THOUGHT_SIGNATURES: LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub fn ensure_root_object_schema(mut schema: serde_json::Value) -> serde_json::Value {
    if let serde_json::Value::Object(ref mut map) = schema {
        if !map.contains_key("type") {
            map.insert("type".to_string(), serde_json::Value::String("object".to_string()));
        }
        if !map.contains_key("properties") {
            map.insert(
                "properties".to_string(),
                serde_json::Value::Object(serde_json::Map::new()),
            );
        }
        schema
    } else {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }
}

pub fn is_valid_thought_signature(signature: Option<&str>) -> bool {
    let Some(sig) = signature else { return false };
    if sig.is_empty() || sig.len() % 4 != 0 {
        return false;
    }
    sig.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
}

pub fn strip_meta_schema(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if !matches!(
                    k.as_str(),
                    "$schema"
                        | "$id"
                        | "$anchor"
                        | "$dynamicAnchor"
                        | "$vocabulary"
                        | "$comment"
                        | "$defs"
                        | "definitions"
                        | "title"
                ) {
                    out.insert(k.clone(), strip_meta_schema(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(arr.iter().map(strip_meta_schema).collect()),
        other => other.clone(),
    }
}

pub fn get_thinking_config(model: &str) -> Option<GeminiThinkingConfig> {
    if model.contains("3.7-flash") || model.contains("3.6-flash") {
        Some(GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: Some("MEDIUM".to_string()),
            thinking_budget: None,
        })
    } else if model.contains("3.5-flash") {
        Some(GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: None,
            thinking_budget: Some(4000),
        })
    } else if model.contains("3.1-pro") {
        Some(GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: None,
            thinking_budget: Some(10001),
        })
    } else {
        None
    }
}

pub fn append_turn(contents: &mut Vec<GeminiContent>, role: &str, mut parts: Vec<GeminiPart>) {
    parts.retain(|p| {
        if let Some(text) = &p.text {
            !text.trim().is_empty()
        } else {
            p.function_call.is_some() || p.function_response.is_some() || p.inline_data.is_some()
        }
    });
    if parts.is_empty() {
        return;
    }
    if let Some(last) = contents.last_mut()
        && last.role == role
    {
        last.parts.extend(parts);
        return;
    }
    contents.push(GeminiContent {
        role: role.to_string(),
        parts,
    });
}

pub fn build_antigravity_request(
    request: &ProviderRequest,
    project_id: &str,
    runtime_model: &str,
) -> AntigravityGenerateRequest {
    let mut contents = Vec::new();
    let mut system_parts = vec![
        GeminiTextPart {
            text: "You are Antigravity, a powerful agentic AI coding assistant designed by Google DeepMind. You are pair programming with a user to solve coding tasks. Be concise, practical, and tool-aware.".to_string(),
        },
        GeminiTextPart {
            text: "CRITICAL: NEVER output rule checks, formatting guidelines, constraint checklists (e.g. \"No emdashes\"), or your thinking/personality preambles in the final response. Output only the final response.".to_string(),
        },
    ];
    let mut call_id_to_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for msg in &request.messages {
        for content in &msg.content {
            if let MessageContent::ToolCall { call_id, tool_id, .. } = content {
                call_id_to_name.insert(call_id.clone(), tool_id.name().to_string());
            }
        }
    }

    for msg in &request.messages {
        match msg.role {
            MessageRole::System => {
                for content in &msg.content {
                    if let MessageContent::Text { text } = content
                        && !text.trim().is_empty()
                    {
                        system_parts.push(GeminiTextPart { text: text.clone() });
                    }
                }
            }
            MessageRole::User => {
                let mut parts = Vec::new();
                for content in &msg.content {
                    if let MessageContent::Text { text } = content
                        && !text.trim().is_empty()
                    {
                        parts.push(GeminiPart {
                            text: Some(text.clone()),
                            thought: None,
                            thought_signature: None,
                            inline_data: None,
                            function_call: None,
                            function_response: None,
                        });
                    }
                }
                append_turn(&mut contents, "user", parts);
            }
            MessageRole::Assistant => {
                let mut parts = Vec::new();
                for content in &msg.content {
                    match content {
                        MessageContent::Text { text } => {
                            if !text.trim().is_empty() {
                                parts.push(GeminiPart {
                                    text: Some(text.clone()),
                                    thought: None,
                                    thought_signature: None,
                                    inline_data: None,
                                    function_call: None,
                                    function_response: None,
                                });
                            }
                        }
                        MessageContent::ToolCall {
                            call_id,
                            tool_id,
                            arguments,
                            ..
                        } => {
                            let sig = THOUGHT_SIGNATURES.lock().unwrap().get(call_id).cloned();
                            if let Some(signature) = sig {
                                parts.push(GeminiPart {
                                    text: None,
                                    thought: None,
                                    thought_signature: Some(signature),
                                    inline_data: None,
                                    function_call: Some(GeminiFunctionCall {
                                        name: tool_id.name().to_string(),
                                        args: arguments.clone(),
                                    }),
                                    function_response: None,
                                });
                            } else {
                                let args_str = serde_json::to_string(arguments).unwrap_or_default();
                                let label = if args_str == "{}" {
                                    format!("`{}`", tool_id.name())
                                } else {
                                    format!("`{}` ({args_str})", tool_id.name())
                                };
                                parts.push(GeminiPart {
                                    text: Some(format!("[Called tool {label}]")),
                                    thought: None,
                                    thought_signature: None,
                                    inline_data: None,
                                    function_call: None,
                                    function_response: None,
                                });
                            }
                        }
                        MessageContent::ToolResult { .. } => {}
                    }
                }
                append_turn(&mut contents, "model", parts);
            }
            MessageRole::Tool => {
                let mut parts = Vec::new();
                for content in &msg.content {
                    if let MessageContent::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } = content
                    {
                        let has_sig = THOUGHT_SIGNATURES.lock().unwrap().contains_key(call_id);
                        let tool_name = call_id_to_name
                            .get(call_id)
                            .cloned()
                            .unwrap_or_else(|| "read".to_string());
                        if has_sig {
                            let response_val = if *is_error {
                                serde_json::json!({ "error": content })
                            } else {
                                serde_json::json!({ "output": content })
                            };
                            parts.push(GeminiPart {
                                text: None,
                                thought: None,
                                thought_signature: None,
                                inline_data: None,
                                function_call: None,
                                function_response: Some(GeminiFunctionResponse {
                                    name: tool_name,
                                    response: response_val,
                                }),
                            });
                        } else {
                            parts.push(GeminiPart {
                                text: Some(format!("[Observation from `{tool_name}`:\n{content}]")),
                                thought: None,
                                thought_signature: None,
                                inline_data: None,
                                function_call: None,
                                function_response: None,
                            });
                        }
                    }
                }
                append_turn(&mut contents, "user", parts);
            }
        }
    }

    if contents.is_empty() {
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart {
                text: Some("Hello".to_string()),
                thought: None,
                thought_signature: None,
                inline_data: None,
                function_call: None,
                function_response: None,
            }],
        });
    } else if contents[0].role != "user" {
        contents.insert(
            0,
            GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: Some("Hello".to_string()),
                    thought: None,
                    thought_signature: None,
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                }],
            },
        );
    }

    let system_instruction = Some(GeminiSystemInstruction {
        role: "user".to_string(),
        parts: system_parts,
    });

    let is_claude = request.model.starts_with("claude-") || runtime_model.starts_with("claude-");
    let tools = if !request.tools.is_empty() {
        let declarations = request
            .tools
            .iter()
            .map(|t| {
                let stripped = strip_meta_schema(&t.argument_schema);
                let root_schema = ensure_root_object_schema(stripped);
                GeminiFunctionDeclaration {
                    name: t.id.name().to_string(),
                    description: Some(t.description.clone()),
                    parameters_json_schema: Some(root_schema),
                    parameters: None,
                }
            })
            .collect();
        Some(vec![GeminiTools {
            function_declarations: declarations,
        }])
    } else {
        None
    };

    let tool_config = if tools.is_some() || is_claude {
        Some(serde_json::json!({
            "functionCallingConfig": {
                "mode": "VALIDATED"
            }
        }))
    } else {
        None
    };

    let generation_config = Some(GeminiGenerationConfig {
        temperature: Some(0.2),
        max_output_tokens: request.max_output_tokens.or(Some(8192)),
        thinking_config: get_thinking_config(&request.model),
    });

    let trajectory_id = uuid::Uuid::new_v4().to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let session_id_int = fastrand::i64(..).abs();
    let labels = serde_json::json!({
        "last_step_index": "1",
        "trajectory_id": trajectory_id,
        "used_claude": if is_claude { "true" } else { "false" },
        "used_claude_conservative": if is_claude { "true" } else { "false" },
    });

    let request_body = AntigravityRequestBody {
        contents,
        system_instruction,
        generation_config,
        tools,
        tool_config,
        session_id: Some(session_id_int.to_string()),
        labels: Some(labels),
    };

    AntigravityGenerateRequest {
        project: project_id.to_string(),
        model: runtime_model.to_string(),
        request: request_body,
        request_type: "agent".to_string(),
        user_agent: "antigravity".to_string(),
        request_id: format!("agent/{}/{}/{}/1", uuid::Uuid::new_v4(), now_ms, trajectory_id),
    }
}
