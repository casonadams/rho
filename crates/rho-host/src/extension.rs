use crate::context::ExtensionContext;
use crate::types::{
    ExtensionCommand, InputAction, PluginCapability, ToolCallDecision, ToolCallEvent, ToolResultEvent, TurnEvent,
};
use async_trait::async_trait;
use rho_core::error::Result;

#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;

    fn capabilities(&self) -> Vec<PluginCapability> {
        PluginCapability::ALL.to_vec()
    }

    async fn on_session_start(&self, _ctx: &ExtensionContext) -> Result<()> {
        Ok(())
    }

    async fn on_session_shutdown(&self, _ctx: &ExtensionContext) -> Result<()> {
        Ok(())
    }

    async fn on_input(&self, _input: &str, _ctx: &ExtensionContext) -> Result<InputAction> {
        Ok(InputAction::Continue)
    }

    async fn before_turn(&self, _event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> Result<()> {
        Ok(())
    }

    async fn on_tool_call(&self, _event: &ToolCallEvent<'_>, _ctx: &ExtensionContext) -> Result<ToolCallDecision> {
        Ok(ToolCallDecision::Allow)
    }

    async fn on_tool_result(&self, _event: &mut ToolResultEvent<'_>, _ctx: &ExtensionContext) -> Result<()> {
        Ok(())
    }

    async fn on_auth_login(&self, _provider: &str, _ctx: &ExtensionContext) -> Result<bool> {
        Ok(false)
    }

    async fn on_auth_logout(&self, _provider: &str, _ctx: &ExtensionContext) -> Result<bool> {
        Ok(false)
    }

    fn register_commands(&self) -> Vec<ExtensionCommand> {
        Vec::new()
    }
}
