//! Model Context Protocol client: child process supervision, JSON-RPC stdio
//! transport, and tool discovery that merges MCP tools into the registry.

pub mod cache;
pub mod client;
pub mod gateway;
pub mod instructions;
pub mod manager;
pub mod naming;
pub mod oauth;
pub mod pool;
pub mod process;
pub mod result;
pub mod runtime;
pub mod search;
#[cfg(test)]
pub(crate) mod test_server;
pub mod transport;
pub mod trust;
pub mod types;

pub use cache::{McpCache, McpCacheEntry, compute_definition_hash, default_cache_path};
pub use client::{
    McpClient, McpContent, McpPromptArgument, McpPromptDefinition, McpPromptMessage, McpPromptResult,
    McpResourceContents, McpResourceDefinition, McpToolDefinition, McpToolResult,
};
pub use instructions::{McpServerInstructions, append_server_instructions};
pub use manager::{McpDiagnostic, McpLoad, load_mcp, load_mcp_tools};
pub use oauth::{McpOAuth, McpTokenStore};
pub use pool::McpServerPool;
pub use process::{McpChildHandle, McpProcess};
pub use runtime::McpRuntime;
pub use search::{McpToolEntry, ServerState, ToolSearchCatalog, build_tool_search_tool, extract_invoked_tool_names};
pub use transport::McpTransport;
pub use trust::is_server_definition_trusted;
pub use types::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
