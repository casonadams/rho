use crate::tools::approval::capability::ApprovalCapability;
use crate::tools::approval::types::{ApprovalDecision, ApprovalRequest, ToolEvent};
use crate::tools::policy::ToolExecutionPolicy;
use rig::tool::{ToolContext, ToolExecutionError};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy)]
pub struct DispatchedCall<'a> {
    pub internal_call_id: &'a str,
    pub tool_name: &'a str,
    pub arguments: &'a Value,
}

/// Run the approval lifecycle for a dispatched call without the rig runner;
/// grants and denials are recorded for [`enforce_approval`] inside the tool body.
pub async fn authorize_dispatch(capability: &ApprovalCapability, call: DispatchedCall<'_>) {
    let DispatchedCall {
        internal_call_id,
        tool_name,
        arguments,
    } = call;
    let class = ToolExecutionPolicy::classify(tool_name, arguments);
    capability.emit_sink(ToolEvent::CallClassified {
        internal_call_id: internal_call_id.to_string(),
        tool_name: tool_name.to_string(),
        arguments: arguments.clone(),
        class: class.clone(),
    });
    if class.allows_without_approval()
        || capability.is_auto_approve()
        || capability.is_session_approved(tool_name, arguments)
    {
        return;
    }
    let crate::tools::policy::ExecutionClass::ApprovalRequired { tier, reasons } = class else {
        return;
    };
    let request = ApprovalRequest {
        tool_name: tool_name.to_string(),
        arguments: arguments.clone(),
        tier,
        reasons,
    };
    match capability.request_approval_sink(request.clone()).await {
        ApprovalDecision::Approved => {
            capability.grant_once(tool_name, arguments);
            capability.emit_sink(ToolEvent::ApprovalGranted {
                internal_call_id: internal_call_id.to_string(),
                tool_name: tool_name.to_string(),
            });
        }
        ApprovalDecision::ApprovedForSession => {
            capability.grant_for_session(tool_name, arguments);
            capability.emit_sink(ToolEvent::ApprovalGranted {
                internal_call_id: internal_call_id.to_string(),
                tool_name: tool_name.to_string(),
            });
        }
        ApprovalDecision::Denied { reason } => {
            capability.deny_once(request, reason);
            capability.emit_sink(ToolEvent::ApprovalDenied {
                internal_call_id: internal_call_id.to_string(),
                tool_name: tool_name.to_string(),
            });
        }
    }
}

/// Terminal result facts for a dispatched call.
pub struct DispatchedResult {
    pub output: String,
    pub status: &'static str,
}

/// Emit the terminal tool-lifecycle event for a dispatched call.
pub fn emit_tool_finished(capability: &ApprovalCapability, call: DispatchedCall<'_>, result: DispatchedResult) {
    capability.emit_sink(ToolEvent::Finished {
        internal_call_id: call.internal_call_id.to_string(),
        tool_name: call.tool_name.to_string(),
        arguments: call.arguments.clone(),
        output: result.output,
        status: result.status.to_string(),
    });
}

/// Build a tool context that carries an [`ApprovalCapability`].
///
/// Tools that mutate state should pull the capability out of the context and
/// call [`enforce_approval`] before doing any work.
pub fn approval_context(capability: ApprovalCapability) -> ToolContext {
    let mut context = ToolContext::new();
    context.insert(capability);
    context
}

/// Enforce that a mutating tool call has been authorized. Read-only and
/// workspace-contained mutations are short-circuited without consulting the
/// approval capability.
pub fn enforce_approval<T>(context: &ToolContext, tool_name: &str, arguments: &T) -> Result<(), ToolExecutionError>
where
    T: Serialize,
{
    let arguments = serde_json::to_value(arguments)
        .map_err(|_| ToolExecutionError::invalid_args("Tool arguments could not be validated safely"))?;
    if ToolExecutionPolicy::classify(tool_name, &arguments).allows_without_approval() {
        return Ok(());
    }

    let capability = context
        .get::<ApprovalCapability>()
        .ok_or_else(|| ToolExecutionError::refused("Approval context is missing; no operation was executed"))?;
    capability.authorize(tool_name, &arguments)
}

/// Build the lookup key used by grants and denials.
///
/// Returns `None` if the policy can't canonicalize the arguments, which is
/// treated as "not approvable" by callers.
pub(crate) fn approval_key(tool_name: &str, arguments: &Value) -> Option<String> {
    let arguments = ToolExecutionPolicy::canonical_arguments(tool_name, arguments)?;
    serde_json::to_string(&arguments)
        .ok()
        .map(|arguments| format!("{tool_name}:{arguments}"))
}

/// Decrement the grant counter for `key`, removing the entry when it hits
/// zero. Returns `false` if there was no grant or the lock was poisoned.
pub(crate) fn consume_grant(grants: &Mutex<HashMap<String, usize>>, key: &str) -> bool {
    let Ok(mut grants) = grants.lock() else {
        return false;
    };
    let Some(count) = grants.get_mut(key) else {
        return false;
    };
    *count -= 1;
    if *count == 0 {
        grants.remove(key);
    }
    true
}
