//! rho plugin SDK: build out-of-process rho plugins over stdio JSON-RPC.
//!
//! Plugins receive [`StepEvent`]s (tool calls, results, lifecycle hooks) and
//! respond with [`Flow`]s that continue, repair, rewrite, or stop the agent.
//! [`HostContext`] exposes host services (UI prompts, notices, status, tool
//! metadata) back to the plugin.

pub mod context;
pub mod serve;
pub mod types;

#[cfg(test)]
mod tests;

pub use context::{HostContext, SelectOption, SelectResult, ToolInfo};
pub use serve::{Plugin, serve, serve_stdio};
pub use types::{Document, Flow, RequestPatch, StepEvent};
