//! Model Context Protocol client: child process supervision, JSON-RPC stdio
//! transport, tool discovery, and the gateway that merges MCP tools into the
//! registry.

pub mod auth;
pub mod cache;
pub mod client;
pub mod gateway;
pub mod manager;
pub mod pool;
pub mod process;
pub mod search;
pub mod transport;
pub mod trust;
pub mod types;

pub use auth::{get_valid_mcp_token, login_mcp_server};
pub use cache::{McpCache, McpCacheEntry, compute_definition_hash, default_cache_path};
pub use client::{
    McpClient, McpContent, McpPromptArgument, McpPromptDefinition, McpPromptMessage, McpResourceContents,
    McpResourceDefinition, McpRoot, McpToolDefinition, McpToolResult,
};
pub use gateway::McpGateway;
pub use manager::{load_mcp_tools, load_mcp_tools_with_activator};
pub use pool::McpServerPool;
pub use process::{McpChildHandle, McpProcess};
pub use search::{
    DeferredMcpTool, DynamicToolActivator, ToolSearchCatalog, build_tool_search_tool, extract_invoked_tool_names,
};
pub use transport::McpTransport;
pub use trust::{
    approve_server_definition, is_server_definition_trusted, is_workspace_trusted, revoke_server_definition,
    trust_workspace, untrust_workspace,
};
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
