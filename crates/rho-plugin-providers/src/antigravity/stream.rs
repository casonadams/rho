use super::client::{HTTP_CLIENT, antigravity_headers, default_project_id, endpoint_candidates, map_runtime_model};
use super::oauth::{load_saved_tokens, refresh_access_token, save_tokens};
use super::types::*;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::BoxStream;
use rho_core::error::{AppError, Result};
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{
    AuthenticationMethod, AuthenticationOperation, AuthenticationRequest, AuthenticationResponse, FinishReason,
    MessageContent, MessageRole, ModelMetadata, ProviderCapability, ProviderDescriptor, ProviderRequest,
    ProviderStreamEvent,
};
use std::path::PathBuf;

pub struct AntigravityProvider {
    config_dir: PathBuf,
}

impl AntigravityProvider {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    fn token_dir(&self) -> PathBuf {
        self.config_dir.join("tokens").join("antigravity")
    }

    pub async fn ensure_valid_tokens(&self) -> Result<AntigravityTokens> {
        let token_dir = self.token_dir();
        let tokens = load_saved_tokens(&token_dir)?.ok_or_else(|| {
            AppError::Auth("No Google Antigravity credentials found. Run 'rho login antigravity'.".to_string())
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        if tokens.expires_at <= now {
            let refreshed = refresh_access_token(&tokens).await?;
            let _ = save_tokens(&token_dir, &refreshed);
            Ok(refreshed)
        } else {
            Ok(tokens)
        }
    }
}

#[async_trait]
impl ProviderCapability for AntigravityProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        let models = vec![
            ModelMetadata {
                id: "gemini-3.7-flash".to_string(),
                display_name: "Gemini 3.7 Flash".to_string(),
                context_limit: Some(1_048_576),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "gemini-3.6-flash".to_string(),
                display_name: "Gemini 3.6 Flash".to_string(),
                context_limit: Some(1_048_576),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "gemini-3.5-flash".to_string(),
                display_name: "Gemini 3.5 Flash".to_string(),
                context_limit: Some(1_048_576),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "gemini-3.1-pro".to_string(),
                display_name: "Gemini 3.1 Pro".to_string(),
                context_limit: Some(1_048_576),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "claude-sonnet-4-6".to_string(),
                display_name: "Claude Sonnet 4.6 (Antigravity)".to_string(),
                context_limit: Some(200_000),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "claude-opus-4-6".to_string(),
                display_name: "Claude Opus 4.6 (Antigravity)".to_string(),
                context_limit: Some(200_000),
                supports_tools: true,
                supports_images: true,
            },
            ModelMetadata {
                id: "gpt-oss-120b".to_string(),
                display_name: "GPT-OSS 120B (Antigravity)".to_string(),
                context_limit: Some(128_000),
                supports_tools: true,
                supports_images: false,
            },
        ];

        ProviderDescriptor {
            id: "provider:antigravity".parse().unwrap(),
            display_name: "Google Antigravity".to_string(),
            models,
            authentication: vec![AuthenticationMethod::OAuth {
                label: "Google Antigravity OAuth".to_string(),
            }],
        }
    }

    async fn authenticate(
        &self,
        request: AuthenticationRequest,
    ) -> std::result::Result<AuthenticationResponse, CapabilityError> {
        match request.operation {
            AuthenticationOperation::Login | AuthenticationOperation::Refresh | AuthenticationOperation::Verify => {
                match self.ensure_valid_tokens().await {
                    Ok(tokens) => Ok(AuthenticationResponse {
                        authenticated: true,
                        refreshed_credential: None,
                        user_message: tokens.email.map(|e| format!("Signed in as {e}")),
                    }),
                    Err(e) => Ok(AuthenticationResponse {
                        authenticated: false,
                        refreshed_credential: None,
                        user_message: Some(e.to_string()),
                    }),
                }
            }
            AuthenticationOperation::Logout => {
                let _ = std::fs::remove_file(self.token_dir().join("auth.json"));
                Ok(AuthenticationResponse {
                    authenticated: false,
                    refreshed_credential: None,
                    user_message: Some("Logged out of Google Antigravity".to_string()),
                })
            }
        }
    }

    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> std::result::Result<
        BoxStream<'static, std::result::Result<ProviderStreamEvent, CapabilityError>>,
        CapabilityError,
    > {
        let tokens = self
            .ensure_valid_tokens()
            .await
            .map_err(|e| CapabilityError::Unavailable { message: e.to_string() })?;

        let project_id = tokens
            .project_id
            .clone()
            .unwrap_or_else(|| default_project_id(tokens.email.as_deref()));

        let runtime_model = map_runtime_model(&request.model);
        let generate_req = build_antigravity_request(&request, &project_id, &runtime_model);

        let stream = try_stream! {
            let mut response_opt = None;
            let mut last_error = String::new();

            for endpoint in endpoint_candidates() {
                let url = format!("{endpoint}/v1internal:streamGenerateContent?alt=sse");
                let headers = antigravity_headers(&tokens.access_token);
                let res = HTTP_CLIENT
                    .post(&url)
                    .headers(headers)
                    .json(&generate_req)
                    .send()
                    .await;

                match res {
                    Ok(response) if response.status().is_success() => {
                        response_opt = Some(response);
                        break;
                    }
                    Ok(response) => {
                        let status = response.status();
                        let text = response.text().await.unwrap_or_default();
                        last_error = format!("Status {status}: {text}");
                    }
                    Err(e) => {
                        last_error = e.to_string();
                    }
                }
            }

            let response = response_opt.ok_or_else(|| CapabilityError::Unavailable {
                message: format!("Antigravity request failed across all endpoints: {last_error}"),
            })?;

            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut had_tool_calls = false;
            let mut terminal_reason = FinishReason::Stop;

            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = chunk_res.map_err(|e| CapabilityError::Unavailable { message: e.to_string() })?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find('\n') {
                    let line = buffer[..pos].trim().to_string();
                    buffer.drain(..=pos);

                    if line.is_empty() || !line.starts_with("data:") {
                        continue;
                    }

                    let json_str = line.trim_start_matches("data:").trim();
                    if json_str.is_empty() || json_str == "[DONE]" {
                        continue;
                    }

                    if let Ok(chunk_data) = serde_json::from_str::<StreamChunkResponse>(json_str) {
                        if let Some(err) = chunk_data.error {
                            Err(CapabilityError::Unavailable {
                                message: format!("Antigravity backend error: {err}"),
                            })?;
                        }

                        let candidates = chunk_data
                            .candidates
                            .or_else(|| chunk_data.response.as_ref().and_then(|r| r.candidates.clone()));

                        if let Some(candidates) = candidates {
                            for cand in candidates {
                                if let Some(content) = cand.content {
                                    for part in content.parts {
                                        if let Some(text) = part.text
                                            && !text.is_empty()
                                        {
                                            yield ProviderStreamEvent::TextDelta { text };
                                        }
                                        if let Some(call) = part.function_call {
                                            had_tool_calls = true;
                                            let call_id = format!("call_{}", uuid::Uuid::new_v4());
                                            let tool_id = CapabilityId::new(rho_sdk::capability::CapabilityKind::Tool, &call.name)
                                                .map_err(|e| CapabilityError::Failed { message: e.to_string() })?;
                                            yield ProviderStreamEvent::ToolCall {
                                                call_id,
                                                tool_id,
                                                arguments: call.args,
                                            };
                                        }
                                    }
                                }
                                if let Some(reason_str) = cand.finish_reason {
                                    terminal_reason = match reason_str.as_str() {
                                        "STOP" => FinishReason::Stop,
                                        "MAX_TOKENS" => FinishReason::Length,
                                        "SAFETY" | "RECITATION" => FinishReason::ContentFilter,
                                        _ => FinishReason::Stop,
                                    };
                                }
                            }
                        }

                        let usage = chunk_data
                            .usage_metadata
                            .or_else(|| chunk_data.response.as_ref().and_then(|r| r.usage_metadata.clone()));

                        if let Some(usage) = usage {
                            yield ProviderStreamEvent::Usage {
                                input_tokens: usage.prompt_tokens.unwrap_or(0),
                                output_tokens: usage.candidates_tokens.unwrap_or(0),
                            };
                        }
                    }
                }
            }

            if had_tool_calls {
                terminal_reason = FinishReason::ToolCalls;
            }

            yield ProviderStreamEvent::Finished {
                reason: terminal_reason,
            };
        };

        Ok(Box::pin(stream))
    }
}

fn append_turn(contents: &mut Vec<GeminiContent>, role: &str, mut parts: Vec<GeminiPart>) {
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
    if let Some(last) = contents.last_mut() {
        if last.role == role {
            last.parts.extend(parts);
            return;
        }
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
    let mut system_parts = Vec::new();
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
                    if let MessageContent::Text { text } = content {
                        if !text.trim().is_empty() {
                            system_parts.push(GeminiTextPart { text: text.clone() });
                        }
                    }
                }
            }
            MessageRole::User => {
                let mut parts = Vec::new();
                for content in &msg.content {
                    if let MessageContent::Text { text } = content {
                        if !text.trim().is_empty() {
                            parts.push(GeminiPart {
                                text: Some(text.clone()),
                                thought: None,
                                inline_data: None,
                                function_call: None,
                                function_response: None,
                            });
                        }
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
                                    inline_data: None,
                                    function_call: None,
                                    function_response: None,
                                });
                            }
                        }
                        MessageContent::ToolCall { tool_id, arguments, .. } => {
                            parts.push(GeminiPart {
                                text: None,
                                thought: None,
                                inline_data: None,
                                function_call: Some(GeminiFunctionCall {
                                    name: tool_id.name().to_string(),
                                    args: arguments.clone(),
                                }),
                                function_response: None,
                            });
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
                        is_error: _,
                    } = content
                    {
                        let tool_name = call_id_to_name
                            .get(call_id)
                            .cloned()
                            .unwrap_or_else(|| "tool".to_string());
                        parts.push(GeminiPart {
                            text: None,
                            thought: None,
                            inline_data: None,
                            function_call: None,
                            function_response: Some(GeminiFunctionResponse {
                                name: tool_name,
                                response: serde_json::json!({ "result": content }),
                            }),
                        });
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
                    inline_data: None,
                    function_call: None,
                    function_response: None,
                }],
            },
        );
    }

    let system_instruction = if !system_parts.is_empty() {
        Some(GeminiSystemInstruction {
            role: "user".to_string(),
            parts: system_parts,
        })
    } else {
        None
    };

    let tools = if !request.tools.is_empty() {
        let declarations = request
            .tools
            .iter()
            .map(|t| GeminiFunctionDeclaration {
                name: t.id.name().to_string(),
                description: Some(t.description.clone()),
                parameters_json_schema: Some(t.argument_schema.clone()),
                parameters: None,
            })
            .collect();
        Some(vec![GeminiTools {
            function_declarations: declarations,
        }])
    } else {
        None
    };

    let generation_config = Some(GeminiGenerationConfig {
        temperature: Some(0.2),
        max_output_tokens: request.max_output_tokens.or(Some(8192)),
        thinking_config: Some(GeminiThinkingConfig {
            include_thoughts: Some(true),
            thinking_level: Some("HIGH".to_string()),
            thinking_budget: None,
        }),
    });

    let request_body = AntigravityRequestBody {
        contents,
        system_instruction,
        generation_config,
        tools,
        tool_config: None,
        session_id: Some(format!("session_{}", uuid::Uuid::new_v4())),
    };

    AntigravityGenerateRequest {
        project: project_id.to_string(),
        model: runtime_model.to_string(),
        request: request_body,
        request_type: "AGENT".to_string(),
        user_agent: "ANTIGRAVITY".to_string(),
        request_id: format!("req_{}", uuid::Uuid::new_v4()),
    }
}
