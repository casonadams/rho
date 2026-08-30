use super::types::{AgentExecutionResult, AgentTemplate};
use rho_core::error::{AppError, Result};
use rho_sdk::contract::{MessageContent, MessageRole, ModelMessage, ProviderCapability, ProviderToolDefinition};
use std::sync::Arc;

pub struct SubagentRunRequest<'a> {
    pub job_id: String,
    pub template: &'a AgentTemplate,
    pub prompt: &'a str,
    pub available_tools: &'a [ProviderToolDefinition],
}

pub struct SubagentRunner {
    pub provider: Arc<dyn ProviderCapability>,
    pub max_turns: usize,
}

impl SubagentRunner {
    pub fn new(provider: Arc<dyn ProviderCapability>, max_turns: usize) -> Self {
        Self { provider, max_turns }
    }

    pub async fn run(&self, request: SubagentRunRequest<'_>) -> Result<AgentExecutionResult> {
        let SubagentRunRequest {
            job_id,
            template,
            prompt,
            available_tools,
        } = request;

        let model = template
            .model
            .clone()
            .unwrap_or_else(|| "claude-3-7-sonnet-20250219".to_string());

        // Filter tools based on template whitelist
        let allowed_tools: Vec<ProviderToolDefinition> = available_tools
            .iter()
            .filter(|t| template.tools.iter().any(|allowed| allowed == t.id.name()))
            .cloned()
            .collect();

        let mut messages = vec![
            ModelMessage {
                role: MessageRole::System,
                content: vec![MessageContent::Text {
                    text: template.system_prompt.clone(),
                }],
            },
            ModelMessage {
                role: MessageRole::User,
                content: vec![MessageContent::Text {
                    text: prompt.to_string(),
                }],
            },
        ];

        let req = rho_sdk::contract::ProviderRequest {
            model,
            messages: messages.clone(),
            credential: None,
            max_output_tokens: None,
            tools: allowed_tools,
        };

        let mut stream = self
            .provider
            .stream(req)
            .await
            .map_err(|e| AppError::Plugin(format!("Subagent stream failed: {e}")))?;

        use futures::StreamExt;
        let mut final_text = String::new();
        let mut tool_calls_count = 0;

        while let Some(event_res) = stream.next().await {
            let event = event_res.map_err(|e| AppError::Plugin(format!("Subagent event error: {e}")))?;
            match event {
                rho_sdk::contract::ProviderStreamEvent::TextDelta { text } => {
                    final_text.push_str(&text);
                }
                rho_sdk::contract::ProviderStreamEvent::ToolCall { .. }
                | rho_sdk::contract::ProviderStreamEvent::ToolCallDelta { .. } => {
                    tool_calls_count += 1;
                }
                _ => {}
            }
        }

        messages.push(ModelMessage {
            role: MessageRole::Assistant,
            content: vec![MessageContent::Text {
                text: final_text.clone(),
            }],
        });

        Ok(AgentExecutionResult {
            job_id,
            status: "completed".to_string(),
            text: final_text,
            tool_calls_count,
            is_error: false,
        })
    }
}

pub struct NoopProvider;

#[async_trait::async_trait]
impl ProviderCapability for NoopProvider {
    fn descriptor(&self) -> rho_sdk::contract::ProviderDescriptor {
        rho_sdk::contract::ProviderDescriptor {
            id: "provider:noop".parse().unwrap(),
            display_name: "Noop".to_string(),
            models: Vec::new(),
            authentication: Vec::new(),
        }
    }

    async fn authenticate(
        &self,
        _request: rho_sdk::contract::AuthenticationRequest,
    ) -> std::result::Result<rho_sdk::contract::AuthenticationResponse, rho_sdk::capability::CapabilityError> {
        unreachable!()
    }

    async fn stream(
        &self,
        _request: rho_sdk::contract::ProviderRequest,
    ) -> std::result::Result<
        futures::stream::BoxStream<
            'static,
            std::result::Result<rho_sdk::contract::ProviderStreamEvent, rho_sdk::capability::CapabilityError>,
        >,
        rho_sdk::capability::CapabilityError,
    > {
        Ok(Box::pin(futures::stream::empty()))
    }
}
