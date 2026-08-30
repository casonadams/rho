pub mod adapter;
pub mod client;
pub mod manager;
pub mod process;
pub mod schema;
pub mod transport;
pub mod types;

pub use adapter::McpToolCapability;
pub use client::{McpClient, McpContent, McpToolDefinition, McpToolResult};
pub use manager::load_mcp_capabilities;
pub use process::{McpChildHandle, McpProcess};
pub use schema::mcp_tool_to_descriptor;
pub use transport::McpTransport;
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
