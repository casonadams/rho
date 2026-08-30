//! Neutral provider-agnostic turn dispatch vocabulary shared by the engine's
//! host loop and the plugin dispatch bridge.

use async_trait::async_trait;
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::ExecutionMode;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NeutralTurnError {
    #[error("provider operation failed")]
    Provider,
    #[error("provider stream is malformed: {0}")]
    Malformed(&'static str),
    #[error("provider requested unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool operation failed: {0}")]
    Tool(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralToolCall {
    pub call_id: String,
    pub tool_id: CapabilityId,
    pub arguments: Value,
}

#[async_trait]
pub trait NeutralToolExecutor: Send + Sync {
    fn execution_mode(&self, _tool_id: &CapabilityId) -> ExecutionMode {
        ExecutionMode::Sequential
    }

    /// Tool definitions advertised to hand-rolled provider implementations.
    fn provider_definitions(&self) -> Vec<rho_sdk::contract::ProviderToolDefinition> {
        Vec::new()
    }

    /// Capture the per-turn tool context (question/stream ports, invocation data).
    async fn begin_turn(&self, _tool_context: &mut rig::tool::ToolContext) {}

    async fn execute(&self, call: NeutralToolCall) -> Result<NeutralToolResult, NeutralTurnError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralToolResult {
    pub content: String,
    pub is_error: bool,
}

pub trait NeutralTurnObserver: Send + Sync {
    fn text_delta(&self, _text: &str) {}
    fn tool_call(&self, _call: &NeutralToolCall) {}
    fn retry(&self, _attempt: u32) {}
}

pub struct NoopTurnObserver;
impl NeutralTurnObserver for NoopTurnObserver {}
