use super::active_set::ActiveToolSet;
use super::types::DispatchContext;
use async_trait::async_trait;
use rho_core::dispatch::{NeutralToolCall, NeutralToolResult, NeutralTurnError};
use rho_sdk::capability::CapabilityId;
use rho_sdk::contract::ExecutionMode;
use rig::tool::ToolContext;
use std::sync::Arc;

pub struct NeutralActiveToolExecutor {
    tools: Arc<ActiveToolSet>,
    context: tokio::sync::Mutex<ToolContext>,
}

impl NeutralActiveToolExecutor {
    pub fn new(tools: Arc<ActiveToolSet>, context: ToolContext) -> Self {
        Self {
            tools,
            context: tokio::sync::Mutex::new(context),
        }
    }
}

#[async_trait]
impl rho_core::dispatch::NeutralToolExecutor for NeutralActiveToolExecutor {
    fn execution_mode(&self, tool_id: &CapabilityId) -> ExecutionMode {
        self.tools.execution_mode(tool_id.name())
    }

    fn provider_definitions(&self) -> Vec<rho_sdk::contract::ProviderToolDefinition> {
        self.tools.provider_definitions()
    }

    async fn begin_turn(&self, tool_context: &mut rig::tool::ToolContext) {
        *self.context.lock().await = tool_context.clone();
    }

    async fn execute(&self, call: NeutralToolCall) -> std::result::Result<NeutralToolResult, NeutralTurnError> {
        let NeutralToolCall {
            call_id: _,
            tool_id,
            arguments,
        } = call;
        let tool = self
            .tools
            .tools
            .get(tool_id.name())
            .ok_or_else(|| NeutralTurnError::UnknownTool(tool_id.to_string()))?;
        let mut context = {
            let guard = self.context.lock().await;
            guard.clone()
        };
        let result = tool
            .dispatch(
                DispatchContext {
                    floor: &self.tools.floor,
                    policies: &self.tools.policies,
                    tool: &mut context,
                },
                arguments,
            )
            .await;
        match result {
            Ok(output) => Ok(NeutralToolResult {
                content: output.as_text().unwrap_or_default().to_string(),
                is_error: false,
            }),
            Err(error) => Ok(NeutralToolResult {
                content: error
                    .model_output()
                    .as_text()
                    .unwrap_or_else(|| error.message())
                    .to_string(),
                is_error: true,
            }),
        }
    }
}
