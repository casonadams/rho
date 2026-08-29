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

pub(crate) mod formatters;
pub(crate) mod renderer;
pub(crate) mod summary;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use formatters::{format_edit_diff, format_thinking_block, format_write_preview};
pub use renderer::{RenderActivity, TerminalRenderer};
pub(crate) use renderer::{format_tool_output_preview, tool_title_style, webfetch_content_kind};
pub use summary::summarize_tool_output;
pub(crate) use summary::{format_tool_args_summary, read_summary_parts};
pub use types::{ApprovalResult, BashApproval, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};

pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{secs}s")
    } else {
        format!("{}ms", duration.as_millis())
    }
}
