use crate::tools::approval::capability::ApprovalCapability;
use crate::tools::approval::capability::OverrideGrant;
use crate::tools::approval::types::{ApprovalDecision, ApprovalRequest, ToolEvent};
use crate::tools::policy::ToolExecutionPolicy;
use rig::agent::hook::{AgentHook, HookContext, ToolCall, ToolCallAction, ToolResultAction, ToolResultEvent};
use serde_json::Value;

/// Hook that intercepts tool calls and routes them through the approval
/// capability. Always returns [`ToolCallAction::Run`] because authorization
/// is enforced again, immediately before the tool body executes.
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
        let class = ToolExecutionPolicy::classify(event.tool_name, &arguments);
        self.capability.emit_sink(ToolEvent::CallClassified {
            internal_call_id: event.internal_call_id.to_string(),
            tool_name: event.tool_name.to_string(),
            arguments: arguments.clone(),
            class: class.clone(),
        });

        if class.allows_without_approval() || self.capability.is_auto_approve() {
            return ToolCallAction::Run;
        }

        let crate::tools::policy::ExecutionClass::ApprovalRequired { tier, reasons } = class else {
            return ToolCallAction::Run;
        };
        let request = ApprovalRequest {
            tool_name: event.tool_name.to_string(),
            arguments: arguments.clone(),
            tier,
            reasons,
        };
        let decision = self.capability.request_approval_sink(request.clone()).await;
        match decision {
            ApprovalDecision::Approved => {
                self.capability.grant_once(event.tool_name, &arguments);
                self.capability.emit_sink(ToolEvent::ApprovalGranted {
                    internal_call_id: event.internal_call_id.to_string(),
                    tool_name: event.tool_name.to_string(),
                });
            }
            ApprovalDecision::ApprovedWithCommand(new_command) => {
                let override_args = serde_json::json!({ "command": new_command });
                self.capability.grant_with_override(OverrideGrant {
                    tool_name: event.tool_name,
                    arguments: &arguments,
                    override_args,
                });
                self.capability.emit_sink(ToolEvent::ApprovalGranted {
                    internal_call_id: event.internal_call_id.to_string(),
                    tool_name: event.tool_name.to_string(),
                });
            }
            ApprovalDecision::Denied { reason } => {
                self.capability.deny_once(request, reason);
                self.capability.emit_sink(ToolEvent::ApprovalDenied {
                    internal_call_id: event.internal_call_id.to_string(),
                    tool_name: event.tool_name.to_string(),
                });
            }
        }
        ToolCallAction::Run
    }

    async fn on_tool_result(&self, _ctx: &HookContext, event: ToolResultEvent<'_>) -> ToolResultAction {
        self.capability.emit_sink(ToolEvent::Finished {
            internal_call_id: event.internal_call_id.to_string(),
            tool_name: event.tool_name.to_string(),
            arguments: serde_json::from_str(event.args).unwrap_or(Value::Null),
            output: event.presentation.render(),
            status: event.raw_result.status_name().to_string(),
        });
        ToolResultAction::Keep
    }
}
