//! Model Context Protocol client: child process supervision, JSON-RPC stdio
//! transport, tool discovery, and the gateway that merges MCP tools into the
//! registry.

pub mod client;
pub mod gateway;
pub mod manager;
pub mod process;
pub mod transport;
pub mod types;

pub use client::{McpClient, McpContent, McpToolDefinition, McpToolResult};
pub use gateway::McpGateway;
pub use manager::load_mcp_tools;
pub use process::{McpChildHandle, McpProcess};
pub use transport::McpTransport;
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
