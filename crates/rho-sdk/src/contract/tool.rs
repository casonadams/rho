use super::validation::{ContractValidationError, ensure_normalized_effects, require_text};
use crate::capability::{CapabilityError, CapabilityId, CapabilityKind};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Sequential,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub id: CapabilityId,
    pub description: String,
    pub argument_schema: Value,
    pub prompt_guidance: String,
    pub effects: Vec<OperationEffect>,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
}

impl ToolDescriptor {
    pub fn validate(&self) -> Result<(), ContractValidationError> {
        self.id.require_kind(CapabilityKind::Tool)?;
        require_text("tool description", &self.description)?;
        ensure_normalized_effects(&self.effects)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationEffect {
    ReadPath { scope: PathScope },
    WritePath { scope: PathScope },
    ExecuteProcess,
    Network { access: NetworkAccess },
    UserInteraction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathScope {
    Workspace,
    Explicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkAccess {
    None,
    PublicInternet,
    ExplicitHosts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvocationContext {
    pub session_id: String,
    pub working_directory: String,
    pub has_interactive_ui: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_config: Option<Value>,
}

impl InvocationContext {
    pub fn new(session_id: impl Into<String>, working_directory: impl Into<String>, has_interactive_ui: bool) -> Self {
        Self {
            session_id: session_id.into(),
            working_directory: working_directory.into(),
            has_interactive_ui,
            plugin_config: None,
        }
    }

    pub fn with_plugin_config(mut self, plugin_config: Option<Value>) -> Self {
        self.plugin_config = plugin_config;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationRequest {
    pub arguments: Value,
    pub context: InvocationContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationResponse {
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRequest {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<InteractionOption>,
    pub allow_custom: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionResponse {
    Selected(usize),
    Custom(String),
    Cancelled,
}

#[async_trait]
pub trait ToolHost: Send + Sync {
    async fn interact(&self, request: InteractionRequest) -> Result<InteractionResponse, CapabilityError>;
    fn stream_chunk(&self, _chunk: &str) {}
    fn progress(&self, _message: &str) {}
}

#[async_trait]
pub trait ToolCapability: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> Result<ToolInvocationResponse, CapabilityError>;
}
