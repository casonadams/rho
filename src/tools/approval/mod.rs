//! Approval gating for mutating tool calls.
//!
//! Splits cleanly into:
//! - [`types`]: data carried through the approval pipeline (request, decision, event).
//! - [`capability`]: the shared, cloneable state held in the [`ToolContext`].
//! - [`hook`]: the [`AgentHook`] that drives approval during a turn.
//! - [`context`]: free functions called from individual tools and the small
//!   helpers that build approval keys, consume grants, and format denials.

mod capability;
mod context;
mod hook;
mod types;

#[cfg(test)]
mod tests;

pub use capability::{ApprovalCapability, format_denial};
pub use context::{
    DispatchedCall, DispatchedResult, approval_context, authorize_dispatch, emit_tool_finished, enforce_approval,
};
pub use hook::ApprovalHook;
pub use types::{ApprovalDecision, ApprovalEventSink, ApprovalRequest, ToolEvent};
