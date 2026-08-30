pub mod activation;
pub mod context;
pub mod extension;
pub mod external;
pub mod loader;
pub mod mcp;
pub mod permission;
pub mod process;
pub mod registry;
pub mod resolver;
pub mod types;

pub use context::ExtensionContext;
pub use extension::Extension;
pub use loader::{PluginDiscovery, PluginLoader};
pub use registry::ExtensionRegistry;
pub use types::{
    CommandHandler, CommandRequest, ExtensionCommand, InputAction, PluginCapability, PluginManifest, ToolCallDecision,
    ToolCallEvent, ToolResultEvent, TurnEvent,
};
