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
pub(crate) mod formatters;
pub(crate) mod notices;
pub(crate) mod presenter;
pub(crate) mod renderer;
pub mod rpc_presenter;

#[cfg(test)]
mod tests;

pub use broadcast_presenter::BroadcastPresenter;
pub(crate) use card::{fetch_content_kind, format_bash_args_header};
pub use formatters::format_relative_time;
pub(crate) use formatters::{format_edit_diff, format_read_expanded, format_thinking_block, format_write_preview};
pub use renderer::{CacheMissNotice, RenderActivity, TerminalRenderer};
pub use rho_harness_core::presentation::summary::summarize_tool_output;
pub(crate) use rho_harness_core::presentation::summary::{format_tool_args_summary, read_summary_parts};
pub use rho_harness_core::presentation::{SessionStatus, ToolLine, ToolOutcome, WelcomeDisplay};
pub use rho_ui_core::{format_duration, format_duration_ms};
pub use rpc_presenter::{PendingApprovals, RpcPresenter};
