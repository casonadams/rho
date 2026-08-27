use crate::plugin::context::ExtensionContext;
use crate::plugin::registry::ExtensionRegistry;
use crate::plugin::types::{ToolCallEvent, ToolResultEvent};
use rig::agent::hook::{
    AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent as RigToolResultEvent,
};
use serde_json::Value;

#[derive(Clone)]
pub struct ExtensionHook {
    registry: ExtensionRegistry,
    context: ExtensionContext,
}

impl ExtensionHook {
    pub fn new(registry: ExtensionRegistry, context: ExtensionContext) -> Self {
        Self { registry, context }
    }
}

impl AgentHook for ExtensionHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let arguments = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        let call_event = ToolCallEvent {
            tool_name: event.tool_name,
            args: &arguments,
        };
        let _ = self.registry.dispatch_tool_call(&call_event, &self.context).await;
        ToolCallAction::Run
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: RigToolResultEvent<'_>) -> ToolResultAction {
        let arguments = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        let mut rendered = event.presentation.render();
        let mut result_event = ToolResultEvent {
            tool_name: event.tool_name,
            args: &arguments,
            result: &mut rendered,
        };
        let _ = self
            .registry
            .dispatch_tool_result(&mut result_event, &self.context)
            .await;
        ToolResultAction::Keep
    }
}
