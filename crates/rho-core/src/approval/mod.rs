//! Host approval contract: classification outcomes, request/decision types,
//! capability glue, and the event sink the presentation layer implements.

pub mod capability;
pub mod context;
pub mod hook;
pub mod types;

pub use capability::{ApprovalCapability, format_denial};
pub use context::{
    DispatchedCall, DispatchedResult, approval_context, authorize_dispatch, emit_tool_finished, enforce_approval,
};
pub use hook::ApprovalHook;
pub use types::{ApprovalDecision, ApprovalEventSink, ApprovalRequest, DENIED_MESSAGE, ToolEvent};
