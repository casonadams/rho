//! Terminal rendering and tool-result formatting.
//!
//! Submodules:
//! - [`renderer`]: the core `TerminalRenderer` struct and its user-facing methods.
//! - [`formatters`]: edit-diff and write-preview formatting.
//!
//! Render payload data and text summarization live in `rho-harness-core`'s
//! presentation module and are re-exported here so external callers continue
//! to use `crate::ui::render::{TerminalRenderer, ToolLine}` etc.

pub(crate) mod card;
pub(crate) mod diff;
pub(crate) mod file_change;
pub(crate) mod formatters;
pub(crate) mod margin;
pub(crate) mod notices;
pub(crate) mod output_highlight;
pub(crate) mod presenter;
pub(crate) mod renderer;
pub(crate) mod thinking;
pub mod turn_stats;

#[cfg(test)]
mod tests;

pub use card::{
    detect_language_from_args, detect_language_from_path, fetch_content_kind, format_bash_args_header,
    format_read_header, format_tool_header, highlight_bash_args_header, normalize_tool_name, render_tool_block,
    render_tool_transcript, tool_title_style,
};
pub use renderer::{NoticeKind, RenderActivity, TerminalRenderer};
pub use rho_harness_core::presentation::summary::summarize_tool_output;
pub use rho_harness_core::presentation::{SessionStatus, ToolLine, WelcomeDisplay};
pub(crate) use thinking::format_thinking_block;

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
