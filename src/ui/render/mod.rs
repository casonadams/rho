//! Terminal rendering, approval prompts, and tool-result formatting.
//!
//! Submodules:
//! - [`renderer`]: the core `TerminalRenderer` struct and its user-facing methods.
//! - [`formatters`]: edit-diff, write-preview, and thinking-block formatting.
//!
//! Render payload data and text summarization live in `rho-harness-core`'s
//! presentation module and are re-exported here so external callers continue
//! to use `crate::ui::render::{TerminalRenderer, ApprovalResult, BashApproval, ToolLine}` etc.

pub(crate) mod card;
pub(crate) mod diff;
pub(crate) mod formatters;
pub(crate) mod notices;
pub(crate) mod presenter;
pub(crate) mod preview;
pub(crate) mod renderer;
pub mod rpc_presenter;

#[cfg(test)]
mod tests;

pub(crate) use formatters::{format_edit_diff, format_thinking_block, format_write_preview};
pub(crate) use preview::{detect_language_from_args, fetch_content_kind, tool_title_style};
pub use renderer::{CacheMissNotice, RenderActivity, TerminalRenderer};
pub use rho_harness_core::presentation::summary::summarize_tool_output;
pub(crate) use rho_harness_core::presentation::summary::{format_tool_args_summary, read_summary_parts};
pub use rho_harness_core::presentation::{
    ApprovalResult, BashApproval, RiskTier, SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay,
};
pub use rpc_presenter::RpcPresenter;

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

/// Formats a tool duration given in milliseconds.
pub fn format_duration_ms(duration_ms: u64) -> String {
    let seconds = duration_ms / 1000;
    let millis = duration_ms % 1000;
    if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else if seconds > 0 {
        if millis == 0 {
            format!("{}s", seconds)
        } else {
            format!("{}.{:03}s", seconds, millis)
        }
    } else {
        format!("{}ms", duration_ms)
    }
}
