use crate::error::Result;
use crate::plugin::context::ExtensionContext;
use crate::plugin::types::{
    ExtensionCommand, InputAction, ToolCallDecision, ToolCallEvent, ToolResultEvent, TurnEvent,
};
use async_trait::async_trait;

#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;

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

    fn register_commands(&self) -> Vec<ExtensionCommand> {
        Vec::new()
    }
}
