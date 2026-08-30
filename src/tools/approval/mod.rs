//! Approval gating for mutating tool calls.
//!
//! Classification, request/decision types, and the capability glue live in
//! `rho-core::approval`; this module retains the rig AgentHook bridge (`hook`)
//! that drives approval during a rig turn.

pub mod hook;

pub use hook::ApprovalHook;
pub use rho_core::approval::{
    ApprovalCapability, ApprovalDecision, ApprovalEventSink, ApprovalRequest, DispatchedCall, DispatchedResult,
    ToolEvent, approval_context, authorize_dispatch, emit_tool_finished, enforce_approval, format_denial,
};
