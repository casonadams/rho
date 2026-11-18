use super::McpTransport;
use rho_harness_core::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct McpResourceContents {
    pub uri: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub blob: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default, rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub resource: Option<McpResourceContents>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpToolResult {
    #[serde(default)]
    pub content: Vec<McpContent>,
    #[serde(default, rename = "isError")]
    pub is_error: Option<bool>,
    #[serde(default, rename = "structuredContent")]
    pub structured_content: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    pub role: String,
    pub content: McpContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptResult {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub messages: Vec<McpPromptMessage>,
}

pub struct McpClient {
    pub server_name: String,
    transport: Arc<McpTransport>,
    instructions: OnceLock<String>,
}

impl McpClient {
    pub fn new(server_name: impl Into<String>, transport: Arc<McpTransport>) -> Self {
        Self {
            server_name: server_name.into(),
            transport,
            instructions: OnceLock::new(),
        }
    }

    pub async fn initialize(&self) -> Result<Value> {
        let params = serde_json::json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "rho",
                "version": env!("CARGO_PKG_VERSION")
            }
        });

        let response = self.transport.request("initialize", Some(params)).await?;
        self.transport.notify("notifications/initialized", None).await?;
        if let Some(text) = response.get("instructions").and_then(Value::as_str)
            && !text.trim().is_empty()
        {
            let _ = self.instructions.set(text.to_string());
        }
        Ok(response)
    }

    pub fn instructions(&self) -> Option<&str> {
        self.instructions.get().map(String::as_str)
    }

    async fn paginate_list<T: serde::de::DeserializeOwned>(&self, endpoint: &str, key: &str) -> Result<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let params = cursor.as_ref().map(|c| serde_json::json!({ "cursor": c }));
            let response = self.transport.request(endpoint, params).await?;

            if let Some(arr) = response.get(key).and_then(|v| v.as_array()) {
                for item in arr {
                    let def: T = serde_json::from_value(item.clone())
                        .map_err(|e| AppError::Mcp(format!("Failed to parse MCP {key} item: {e}")))?;
                    items.push(def);
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

        Ok(items)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolDefinition>> {
        self.paginate_list("tools/list", "tools").await
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpToolResult> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });

        let response = self.transport.request("tools/call", Some(params)).await?;

        serde_json::from_value(response).map_err(|e| AppError::Mcp(format!("Failed to parse MCP tool result: {e}")))
    }

    pub async fn list_resources(&self) -> Result<Vec<McpResourceDefinition>> {
        self.paginate_list("resources/list", "resources").await
    }

    pub async fn list_prompts(&self) -> Result<Vec<McpPromptDefinition>> {
        self.paginate_list("prompts/list", "prompts").await
    }

    pub async fn read_resource(&self, uri: &str) -> Result<Vec<McpResourceContents>> {
        let response = self
            .transport
            .request("resources/read", Some(serde_json::json!({ "uri": uri })))
            .await?;
        let contents = response.get("contents").cloned().unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(contents)
            .map_err(|e| AppError::Mcp(format!("Failed to parse MCP resource contents: {e}")))
    }

    pub async fn get_prompt(&self, name: &str, arguments: &BTreeMap<String, String>) -> Result<McpPromptResult> {
        let params = serde_json::json!({ "name": name, "arguments": arguments });
        let response = self.transport.request("prompts/get", Some(params)).await?;
        serde_json::from_value(response).map_err(|e| AppError::Mcp(format!("Failed to parse MCP prompt: {e}")))
    }
}

#[cfg(test)]
mod tests;
