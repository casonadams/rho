//! Terminal rendering and tool-result formatting.
//!
//! Submodules:
//! - [`renderer`]: the core `TerminalRenderer` struct and its user-facing methods.
//! - [`formatters`]: edit-diff, write-preview, and thinking-block formatting.
//!
//! Render payload data and text summarization live in `rho-harness-core`'s
//! presentation module and are re-exported here so external callers continue
//! to use `crate::ui::render::{TerminalRenderer, ToolLine}` etc.

pub mod broadcast_presenter;
pub(crate) mod card;
pub(crate) mod diff;
pub(crate) mod formatters;
pub(crate) mod notices;
pub(crate) mod presenter;
pub(crate) mod renderer;
pub mod rpc_presenter;

#[cfg(test)]
mod tests;

pub use broadcast_presenter::BroadcastPresenter;
pub use card::{
    detect_language_from_args, detect_language_from_path, fetch_content_kind, format_bash_args_header,
    format_read_header, format_tool_header, normalize_tool_name, render_headless_tool_card, render_tool_block,
    render_tool_transcript, tool_title_style,
};
pub(crate) use formatters::format_thinking_block;
pub use renderer::{CacheMissNotice, RenderActivity, TerminalRenderer};
pub use rho_harness_core::presentation::summary::summarize_tool_output;
pub use rho_harness_core::presentation::{SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
pub use rpc_presenter::{PendingApprovals, RpcPresenter};

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
