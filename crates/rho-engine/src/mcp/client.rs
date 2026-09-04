use super::McpTransport;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: Option<bool>,
}

impl McpToolResult {
    pub fn as_text(&self) -> String {
        self.as_text_truncated(usize::MAX)
    }

    pub fn as_text_truncated(&self, max_bytes: usize) -> String {
        let mut out = String::new();
        for item in &self.content {
            if let Some(text) = &item.text {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            } else if item.kind == "image" {
                if !out.is_empty() {
                    out.push('\n');
                }
                let mime = item.mime_type.as_deref().unwrap_or("image/png");
                let data_len = item.data.as_ref().map(|d| d.len()).unwrap_or(0);
                out.push_str(&format!("[Image: {mime}, {data_len} bytes base64]"));
            }
        }
        if out.len() > max_bytes && max_bytes > 0 {
            let truncated = &out[..out.floor_char_boundary(max_bytes.min(out.len()))];
            format!("{truncated}\n[MCP tool output truncated at {max_bytes} bytes]")
        } else {
            out
        }
    }
}

pub struct McpClient {
    pub server_name: String,
    transport: Arc<McpTransport>,
}

impl McpClient {
    pub fn new(server_name: impl Into<String>, transport: Arc<McpTransport>) -> Self {
        Self {
            server_name: server_name.into(),
            transport,
        }
    }

    pub async fn initialize(&self) -> Result<Value> {
        let params = serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "clientInfo": {
                "name": "rho",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let response = self.transport.request("initialize", Some(params)).await?;

        // Send notifications/initialized
        self.transport.notify("notifications/initialized", None).await?;

        Ok(response)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let mut all_tools = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = cursor.as_ref().map(|c| serde_json::json!({ "cursor": c }));
            let response = self.transport.request("tools/list", params).await?;

            if let Some(tools_arr) = response.get("tools").and_then(|v| v.as_array()) {
                for tool_val in tools_arr {
                    let tool_def: McpToolDefinition = serde_json::from_value(tool_val.clone())
                        .map_err(|e| AppError::Plugin(format!("Failed to parse MCP tool definition: {e}")))?;
                    all_tools.push(tool_def);
                }
            }

            if let Some(next_cursor) = response.get("nextCursor").and_then(|v| v.as_str())
                && !next_cursor.is_empty()
            {
                cursor = Some(next_cursor.to_string());
                continue;
            }
            break;
        }

        Ok(all_tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let response = self.transport.request("tools/call", Some(params)).await?;

        serde_json::from_value(response).map_err(|e| AppError::Plugin(format!("Failed to parse MCP tool result: {e}")))
    }
}

#[cfg(test)]
mod tests;
