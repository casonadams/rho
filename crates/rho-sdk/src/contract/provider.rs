use super::validation::{ContractValidationError, require_text};
use crate::capability::{CapabilityError, CapabilityId, CapabilityKind};
use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{Debug, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub id: CapabilityId,
    pub display_name: String,
    pub models: Vec<ModelMetadata>,
    pub authentication: Vec<AuthenticationMethod>,
}

impl ProviderDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Provider)?;
        require_text("provider display name", &self.display_name)?;
        let mut model_ids = std::collections::BTreeSet::new();
        for model in &self.models {
            require_text("model identifier", &model.id)?;
            require_text("model display name", &model.display_name)?;
            if !model_ids.insert(&model.id) {
                return Err(ContractValidationError::DuplicateModel(model.id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub id: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_limit: Option<u64>,
    pub supports_tools: bool,
    pub supports_images: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthenticationMethod {
    None,
    ApiKey { label: String },
    OAuth { label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationRequest {
    pub operation: AuthenticationOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<ScopedCredential>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationOperation {
    Login,
    Refresh,
    Verify,
    Logout,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedCredential {
    pub kind: String,
    pub value: Value,
}

impl Debug for ScopedCredential {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScopedCredential")
            .field("kind", &self.kind)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationResponse {
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed_credential: Option<ScopedCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ModelMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<ScopedCredential>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    pub tools: Vec<ProviderToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: MessageRole,
    pub content: Vec<MessageContent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    ToolCall {
        call_id: String,
        tool_id: CapabilityId,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderToolDefinition {
    pub id: CapabilityId,
    pub description: String,
    pub argument_schema: Value,
}

impl ProviderToolDefinition {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Tool)?;
        require_text("tool description", &self.description)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderStreamEvent {
    TextDelta {
        text: String,
    },
    ToolCallDelta {
        call_id: String,
        tool_id: CapabilityId,
        arguments_delta: String,
    },
    ToolCall {
        call_id: String,
        tool_id: CapabilityId,
        arguments: Value,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Finished {
        reason: FinishReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    Length,
    ContentFilter,
    Cancelled,
}

#[async_trait]
pub trait ProviderCapability: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    async fn authenticate(&self, request: AuthenticationRequest) -> Result<AuthenticationResponse, CapabilityError>;
    async fn stream(
        &self,
        request: ProviderRequest,
    ) -> Result<BoxStream<'static, Result<ProviderStreamEvent, CapabilityError>>, CapabilityError>;
}
