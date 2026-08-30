use super::context::{approval_key, consume_grant};
use super::types::{ApprovalEventSink, ApprovalRequest, DENIED_MESSAGE, ToolEvent};
use crate::policy::{ExecutionClass, ToolExecutionPolicy};
use async_trait::async_trait;
use rho_sdk::capability::{CapabilityError, CapabilityId};
use rho_sdk::contract::{PermissionCapability, PermissionDecision, RequestedOperation};
use rig::tool::ToolExecutionError;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

struct ApprovalCapabilityInner {
    auto_approve: bool,
    sink: Arc<dyn ApprovalEventSink>,
    grants: Mutex<HashMap<String, usize>>,
    session_grants: Arc<Mutex<HashSet<String>>>,
    denials: Mutex<HashMap<String, String>>,
}

/// Cloneable handle to the shared approval state stored in the tool context.
#[derive(Clone)]
pub struct ApprovalCapability {
    inner: Arc<ApprovalCapabilityInner>,
}

impl ApprovalCapability {
    pub fn new(auto_approve: bool, sink: Arc<dyn ApprovalEventSink>) -> Self {
        Self::with_session_grants(auto_approve, sink, Arc::new(Mutex::new(HashSet::new())))
    }

    pub fn with_session_grants(
        auto_approve: bool,
        sink: Arc<dyn ApprovalEventSink>,
        session_grants: Arc<Mutex<HashSet<String>>>,
    ) -> Self {
        Self {
            inner: Arc::new(ApprovalCapabilityInner {
                auto_approve,
                sink,
                grants: Mutex::new(HashMap::new()),
                session_grants,
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

    pub fn grant_for_session(&self, tool_name: &str, arguments: &Value) {
        let Some(patterns) = session_patterns(tool_name, arguments) else {
            return;
        };
        if let Ok(mut grants) = self.inner.session_grants.lock() {
            grants.extend(patterns);
        }
    }

    pub fn is_session_approved(&self, tool_name: &str, arguments: &Value) -> bool {
        let Some(patterns) = session_patterns(tool_name, arguments) else {
            return false;
        };
        self.inner
            .session_grants
            .lock()
            .is_ok_and(|grants| patterns.iter().all(|pattern| grants.contains(pattern)))
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
        if consume_grant(&self.inner.grants, &key) || self.is_session_approved(tool_name, arguments) {
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

    /// Built-in default permission decision for a normalized operation:
    /// allow what the host classifies as safe, auto-approved, or
    /// session-granted; require approval for everything else.
    pub fn decision(&self, operation: &RequestedOperation, class: &ExecutionClass) -> PermissionDecision {
        if class.allows_without_approval()
            || self.inner.auto_approve
            || self.is_session_approved(operation.tool_id.name(), &operation.arguments)
        {
            return PermissionDecision::Allow;
        }
        let ExecutionClass::ApprovalRequired { reasons, .. } = class else {
            return PermissionDecision::Allow;
        };
        PermissionDecision::ApprovalRequired {
            rationale: reasons.join("; "),
        }
    }
}

#[async_trait]
impl PermissionCapability for ApprovalCapability {
    fn id(&self) -> CapabilityId {
        "permission:default".parse().expect("default permission id is valid")
    }

    async fn evaluate(&self, request: RequestedOperation) -> Result<PermissionDecision, CapabilityError> {
        let class = ToolExecutionPolicy::classify(request.tool_id.name(), &request.arguments);
        Ok(self.decision(&request, &class))
    }
}

fn session_patterns(tool_name: &str, arguments: &Value) -> Option<Vec<String>> {
    if tool_name != "bash" {
        return None;
    }
    let command = arguments.get("command")?.as_str()?;
    let patterns = crate::bash_ast::analyze_command_safety(command).session_patterns?;
    (!patterns.is_empty()).then_some(patterns)
}

pub fn format_denial(reason: String) -> String {
    let trimmed = reason.trim().trim_start_matches("Denied by user:").trim();
    if trimmed.is_empty() || trimmed == "Execution denied by user." || trimmed == "Execution canceled by user." {
        DENIED_MESSAGE.to_string()
    } else {
        format!("Operation denied by user: {trimmed}. No changes were made.")
    }
}
