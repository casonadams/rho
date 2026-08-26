use crate::tools::approval::capability::ApprovalCapability;
use crate::tools::policy::ToolExecutionPolicy;
use rig::tool::{ToolContext, ToolExecutionError};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

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
