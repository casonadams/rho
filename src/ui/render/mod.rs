//! Terminal rendering, approval prompts, and tool-result formatting.
//!
//! Submodules:
//! - [`types`]: data types used across the module (`ApprovalResult`, etc.).
//! - [`renderer`]: the core `TerminalRenderer` struct and its user-facing methods.
//! - [`formatters`]: edit-diff, write-preview, and thinking-block formatting.
//! - [`summary`]: path cleaning, tool-arg summarization, and bash-approval helpers.
//!
//! Public API is re-exported here so external callers continue to use
//! `crate::ui::render::{TerminalRenderer, ApprovalResult, BashApproval, ToolLine}` etc.

mod formatters;
mod renderer;
mod summary;
mod types;

#[cfg(test)]
mod tests;

pub use renderer::TerminalRenderer;
pub use summary::summarize_tool_output;
pub use types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
