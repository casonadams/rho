use crate::tools::approval::context::{approval_key, consume_grant};
use crate::tools::approval::types::{ApprovalEventSink, ApprovalRequest, DENIED_MESSAGE, ToolEvent};
use rig::tool::ToolExecutionError;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Borrow describing a one-shot approval that should also rewrite the
/// arguments before the tool executes (the "edit then approve" path).
pub(crate) struct OverrideGrant<'a> {
    pub tool_name: &'a str,
    pub arguments: &'a Value,
    pub override_args: Value,
}

struct ApprovalCapabilityInner {
    auto_approve: bool,
    sink: Arc<dyn ApprovalEventSink>,
    grants: Mutex<HashMap<String, usize>>,
    overrides: Mutex<HashMap<String, Value>>,
    denials: Mutex<HashMap<String, String>>,
}

/// Cloneable handle to the shared approval state stored in the tool context.
#[derive(Clone)]
pub struct ApprovalCapability {
    inner: Arc<ApprovalCapabilityInner>,
}

impl ApprovalCapability {
    pub fn new(auto_approve: bool, sink: Arc<dyn ApprovalEventSink>) -> Self {
        Self {
            inner: Arc::new(ApprovalCapabilityInner {
                auto_approve,
                sink,
                grants: Mutex::new(HashMap::new()),
                overrides: Mutex::new(HashMap::new()),
                denials: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// `true` when all mutating tool calls should run without prompting.
    pub fn is_auto_approve(&self) -> bool {
        self.inner.auto_approve
    }

    /// Forward a lifecycle event to the configured sink.
    pub fn emit_sink(&self, event: ToolEvent) {
        self.inner.sink.emit(event);
    }

    /// Ask the configured sink to approve a pending request.
    pub async fn request_approval_sink(&self, request: ApprovalRequest) -> super::types::ApprovalDecision {
        self.inner.sink.request_approval(request).await
    }

    /// Record a one-shot approval for the given tool call.
    pub fn grant_once(&self, tool_name: &str, arguments: &Value) {
        let Some(key) = approval_key(tool_name, arguments) else {
            return;
        };
        if let Ok(mut grants) = self.inner.grants.lock() {
            *grants.entry(key).or_default() += 1;
        }
    }

    /// Record a one-shot approval that also overrides the tool's arguments.
    pub(crate) fn grant_with_override(&self, grant: OverrideGrant<'_>) {
        let Some(key) = approval_key(grant.tool_name, grant.arguments) else {
            return;
        };
        if let Ok(mut grants) = self.inner.grants.lock() {
            *grants.entry(key.clone()).or_default() += 1;
        }
        if let Ok(mut overrides) = self.inner.overrides.lock() {
            overrides.insert(key, grant.override_args);
        }
    }

    /// Pop any pending argument override for the given tool call.
    pub fn take_override(&self, tool_name: &str, arguments: &Value) -> Option<Value> {
        let key = approval_key(tool_name, arguments)?;
        self.inner.overrides.lock().ok()?.remove(&key)
    }

    /// Record a denial that will short-circuit the next matching call.
    pub fn deny_once(&self, request: ApprovalRequest, reason: String) {
        let Some(key) = approval_key(&request.tool_name, &request.arguments) else {
            return;
        };
        if let Ok(mut denials) = self.inner.denials.lock() {
            denials.insert(key, format_denial(reason));
        }
    }

    /// Authorize a tool call, consuming any pending grant or denial.
    pub fn authorize(&self, tool_name: &str, arguments: &Value) -> Result<(), ToolExecutionError> {
        if self.inner.auto_approve {
            return Ok(());
        }
        let Some(key) = approval_key(tool_name, arguments) else {
            return Err(ToolExecutionError::refused(DENIED_MESSAGE));
        };
        if consume_grant(&self.inner.grants, &key) {
            return Ok(());
        }
        let reason = self
            .inner
            .denials
            .lock()
            .ok()
            .and_then(|mut denials| denials.remove(&key))
            .unwrap_or_else(|| DENIED_MESSAGE.to_string());
        Err(ToolExecutionError::refused(reason))
    }
}

fn format_denial(reason: String) -> String {
    let trimmed = reason.trim().trim_start_matches("Denied by user:").trim();
    if trimmed.is_empty() || trimmed == "Execution denied by user." || trimmed == "Execution canceled by user." {
        DENIED_MESSAGE.to_string()
    } else {
        format!("Operation denied by user: {trimmed}. No changes were made.")
    }
}
