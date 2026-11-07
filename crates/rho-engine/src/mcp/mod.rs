//! Model Context Protocol client: child process supervision, JSON-RPC stdio
//! transport, tool discovery, and the gateway that merges MCP tools into the
//! registry.

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

pub use cache::{McpCache, McpCacheEntry, compute_definition_hash, default_cache_path};
pub use client::{
    McpClient, McpContent, McpPromptArgument, McpPromptDefinition, McpResourceDefinition, McpToolDefinition,
    McpToolResult,
};
pub use gateway::McpGateway;
pub use manager::{load_mcp_tools, load_mcp_tools_with_activator};
pub use pool::McpServerPool;
pub use process::{McpChildHandle, McpProcess};
pub use search::{
    DeferredMcpTool, DynamicToolActivator, ToolSearchCatalog, build_tool_search_tool, extract_invoked_tool_names,
};
pub use transport::McpTransport;
pub use trust::is_server_definition_trusted;
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
