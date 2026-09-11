use super::McpTransport;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
pub const MCP_PROTOCOL_VERSION_FALLBACK: &str = "2024-11-05";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceDefinition {
    pub uri: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContents {
    pub uri: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blob: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: McpContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRoot {
    pub uri: String,
    #[serde(default)]
    pub name: Option<String>,
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

fn format_mcp_content_item(item: &McpContent) -> Option<String> {
    if let Some(text) = &item.text {
        Some(text.clone())
    } else if item.kind == "image" {
        let mime = item.mime_type.as_deref().unwrap_or("image/png");
        let data_len = item.data.as_ref().map(|d| d.len()).unwrap_or(0);
        Some(format!("[Image: {mime}, {data_len} bytes base64]"))
    } else {
        None
    }
}

fn truncate_mcp_text(out: &str, max_bytes: usize) -> String {
    if out.len() > max_bytes && max_bytes > 0 {
        let truncated = &out[..out.floor_char_boundary(max_bytes.min(out.len()))];
        format!("{truncated}\n[MCP tool output truncated at {max_bytes} bytes]")
    } else {
        out.to_string()
    }
}

impl McpToolResult {
    pub fn as_text(&self) -> String {
        self.as_text_truncated(usize::MAX)
    }

    pub fn as_text_truncated(&self, max_bytes: usize) -> String {
        let lines: Vec<String> = self.content.iter().filter_map(format_mcp_content_item).collect();
        let out = lines.join("\n");
        truncate_mcp_text(&out, max_bytes)
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
                "tools": { "listChanged": true },
                "resources": { "subscribe": false, "listChanged": true },
                "prompts": { "listChanged": true },
                "roots": { "listChanged": false }
            },
            "clientInfo": {
                "name": "rho",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let response = self.transport.request("initialize", Some(params)).await?;
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

    pub async fn list_resources(&self) -> Result<Vec<McpResourceDefinition>> {
        let mut all_resources = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = cursor.as_ref().map(|c| serde_json::json!({ "cursor": c }));
            let response = self.transport.request("resources/list", params).await?;

            if let Some(arr) = response.get("resources").and_then(|v| v.as_array()) {
                for item in arr {
                    let def: McpResourceDefinition = serde_json::from_value(item.clone())
                        .map_err(|e| AppError::Plugin(format!("Failed to parse resource definition: {e}")))?;
                    all_resources.push(def);
                }
            }

            if let Some(next) = response.get("nextCursor").and_then(|v| v.as_str())
                && !next.is_empty()
            {
                cursor = Some(next.to_string());
                continue;
            }
            break;
        }

        Ok(all_resources)
    }

    pub async fn read_resource(&self, uri: &str) -> Result<Vec<McpResourceContents>> {
        let params = serde_json::json!({ "uri": uri });
        let response = self.transport.request("resources/read", Some(params)).await?;

        let contents_val = response.get("contents").cloned().unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(contents_val)
            .map_err(|e| AppError::Plugin(format!("Failed to parse resource contents: {e}")))
    }

    pub async fn list_prompts(&self) -> Result<Vec<McpPromptDefinition>> {
        let mut all_prompts = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = cursor.as_ref().map(|c| serde_json::json!({ "cursor": c }));
            let response = self.transport.request("prompts/list", params).await?;

            if let Some(arr) = response.get("prompts").and_then(|v| v.as_array()) {
                for item in arr {
                    let def: McpPromptDefinition = serde_json::from_value(item.clone())
                        .map_err(|e| AppError::Plugin(format!("Failed to parse prompt definition: {e}")))?;
                    all_prompts.push(def);
                }
            }

            if let Some(next) = response.get("nextCursor").and_then(|v| v.as_str())
                && !next.is_empty()
            {
                cursor = Some(next.to_string());
                continue;
            }
            break;
        }

        Ok(all_prompts)
    }

    pub async fn get_prompt(&self, name: &str, arguments: Option<Value>) -> Result<Vec<McpPromptMessage>> {
        let mut params = serde_json::json!({ "name": name });
        if let Some(args) = arguments {
            params["arguments"] = args;
        }
        let response = self.transport.request("prompts/get", Some(params)).await?;

        let messages_val = response.get("messages").cloned().unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(messages_val)
            .map_err(|e| AppError::Plugin(format!("Failed to parse prompt messages: {e}")))
    }
}

#[cfg(test)]
mod tests;
