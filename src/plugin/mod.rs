//! Plugin platform facade: every module lives in `rho-host`, with the
//! dispatch-boundary types surfaced from the SDK (`rho-sdk` capability,
//! contract, protocol, and schema modules keep their `crate::plugin::` paths
//! through re-exports).
//!
//! Paths preserved for consumers: `crate::plugin::{activation, builtin,
//! builtin_tools, contract, inspection, mcp, process, safety_floor, tool_dispatch, ...}`.

pub use rho_host::context::ExtensionContext;
pub use rho_host::extension::Extension;
pub use rho_host::registry::ExtensionRegistry;
pub use rho_host::types::{
    CommandHandler, CommandRequest, ExtensionCommand, InputAction, PluginCapability, PluginManifest, ToolCallDecision,
    ToolCallEvent, ToolResultEvent, TurnEvent,
};
pub use rho_host::{
    activation, context, extension, external, loader, mcp, permission, process, registry, resolver, types,
};
pub use rho_sdk::{capability, contract, protocol, schema};

pub mod builtin;
pub mod builtin_tools;
pub mod inspection;
pub mod safety_floor;
pub mod tool_dispatch;
