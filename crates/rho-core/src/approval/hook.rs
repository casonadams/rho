use super::ApprovalCapability;
use super::{DispatchedCall, DispatchedResult, authorize_dispatch, emit_tool_finished};
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent};
use serde_json::Value;

/// Hook that intercepts tool calls and routes them through the approval
/// capability. Always returns [`ToolCallAction::Run`] because authorization
/// is enforced again, immediately before the tool body executes.
///
/// Retained only for direct-rig scenarios (the evaluation harness and tests
/// that build raw Rig agents). Production sequencing lives in the
/// [`crate::plugin::tool_dispatch::ActiveToolSet`] dispatch boundary; do not
/// wire this hook there, it would duplicate approvals and lifecycle events.
#[derive(Clone)]
pub struct ApprovalHook {
    capability: ApprovalCapability,
}

impl ApprovalHook {
    pub fn new(capability: ApprovalCapability) -> Self {
        Self { capability }
    }
}

impl AgentHook for ApprovalHook {
    async fn on_tool_call(&self, _ctx: &HookContext, event: ToolCall<'_>) -> ToolCallAction {
        let arguments = serde_json::from_str::<Value>(event.args).unwrap_or(Value::Null);
        authorize_dispatch(
            &self.capability,
            DispatchedCall {
                internal_call_id: event.internal_call_id,
                tool_name: event.tool_name,
                arguments: &arguments,
            },
        )
        .await;
        ToolCallAction::Run
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        let arguments = serde_json::from_str(event.args).unwrap_or(Value::Null);
        emit_tool_finished(
            &self.capability,
            DispatchedCall {
                internal_call_id: event.internal_call_id,
                tool_name: event.tool_name,
                arguments: &arguments,
            },
            DispatchedResult {
                output: event.presentation.render(),
                status: event.raw_result.status_name(),
            },
        );
        ToolResultAction::Keep
    }
}
