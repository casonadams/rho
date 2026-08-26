use crate::tools::bash_ast::RiskTier;
use async_trait::async_trait;
use serde_json::Value;

/// Message returned when an operation is denied without a user-provided reason.
pub(crate) const DENIED_MESSAGE: &str = "Operation denied by user; no changes were made.";

/// A pending request for user approval, emitted before a mutating tool runs.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: Value,
    pub tier: RiskTier,
    pub reasons: Vec<String>,
}

/// Outcome of asking the user (or a programmatic policy) whether a tool may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    ApprovedWithCommand(String),
    Denied { reason: String },
}

/// Observability events emitted by the approval pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolEvent {
    CallClassified {
        internal_call_id: String,
        tool_name: String,
        arguments: Value,
        class: crate::tools::policy::ExecutionClass,
    },
    ApprovalGranted {
        internal_call_id: String,
        tool_name: String,
    },
    ApprovalDenied {
        internal_call_id: String,
        tool_name: String,
    },
    Finished {
        internal_call_id: String,
        tool_name: String,
        arguments: Value,
        output: String,
        status: String,
    },
}

/// Sink for approval requests and tool lifecycle events.
///
/// Implementations decide how to prompt the user (or apply a programmatic
/// policy) and how to surface events to the UI.
#[async_trait]
pub trait ApprovalEventSink: Send + Sync {
    async fn request_approval(&self, request: ApprovalRequest) -> ApprovalDecision;

    fn emit(&self, _event: ToolEvent) {}
}
