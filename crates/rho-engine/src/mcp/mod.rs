//! Model Context Protocol client: child process supervision, JSON-RPC stdio
//! transport, tool discovery, and the gateway that merges MCP tools into the
//! registry.

pub mod auth;
pub mod client;
pub mod gateway;
pub mod manager;
pub mod process;
pub mod transport;
pub mod trust;
pub mod types;

pub use auth::{get_valid_mcp_token, login_mcp_server};
pub use client::{
    McpClient, McpContent, McpPromptArgument, McpPromptDefinition, McpPromptMessage, McpResourceContents,
    McpResourceDefinition, McpRoot, McpToolDefinition, McpToolResult,
};
pub use gateway::McpGateway;
pub use manager::load_mcp_tools;
pub use process::{McpChildHandle, McpProcess};
pub use transport::McpTransport;
pub use trust::{is_workspace_trusted, trust_workspace, untrust_workspace};
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
