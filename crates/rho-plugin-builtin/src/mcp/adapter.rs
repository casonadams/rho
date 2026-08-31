use super::client::McpClient;
use async_trait::async_trait;
use rho_sdk::capability::CapabilityError;
use rho_sdk::contract::{ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse};
use std::sync::Arc;

pub struct McpToolCapability {
    client: Arc<McpClient>,
    tool_name: String,
    descriptor: ToolDescriptor,
}

impl McpToolCapability {
    pub fn new(client: Arc<McpClient>, tool_name: impl Into<String>, descriptor: ToolDescriptor) -> Self {
        Self {
            client,
            tool_name: tool_name.into(),
            descriptor,
        }
    }
}

#[async_trait]
impl ToolCapability for McpToolCapability {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn invoke(
        &self,
        _host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> std::result::Result<ToolInvocationResponse, CapabilityError> {
        let mcp_res = self
            .client
            .call_tool(&self.tool_name, request.arguments)
            .await
            .map_err(|error| CapabilityError::Failed {
                message: format!("MCP tool '{}' invocation failed: {error}", self.tool_name),
            })?;

        Ok(ToolInvocationResponse {
            content: mcp_res.as_text(),
            is_error: mcp_res.is_error.unwrap_or(false),
            structured_content: None,
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::mcp::process::McpProcess;
    use crate::mcp::transport::McpTransport;
    use rho_core::config::McpServerConfig;
    use rho_sdk::contract::ExecutionMode;
    use std::collections::BTreeMap;

    struct DummyToolHost;
    #[async_trait]
    impl ToolHost for DummyToolHost {
        async fn interact(
            &self,
            _request: rho_sdk::contract::InteractionRequest,
        ) -> std::result::Result<rho_sdk::contract::InteractionResponse, CapabilityError> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn test_mcp_tool_capability_invoke() {
        let script = r#"
read line
id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello from tool\"}],\"isError\":false}}"
"#;
        let config = McpServerConfig {
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), script.to_string()],
            env: BTreeMap::new(),
            enabled: true,
        };

        let (stdin, stdout, handle) = McpProcess::spawn(&config, &std::env::temp_dir()).unwrap();
        let transport = McpTransport::new(stdin, stdout, handle);
        let client = Arc::new(McpClient::new("test-server", transport));

        let descriptor = ToolDescriptor {
            id: "tool:echo".parse().unwrap(),
            description: "echo tool".to_string(),
            argument_schema: serde_json::json!({"type": "object"}),
            prompt_guidance: String::new(),
            effects: Vec::new(),
            execution_mode: ExecutionMode::Sequential,
        };

        let tool_cap = McpToolCapability::new(client, "echo", descriptor);
        let host = DummyToolHost;
        let response = tool_cap
            .invoke(
                &host,
                ToolInvocationRequest {
                    arguments: serde_json::json!({}),
                    context: rho_sdk::contract::InvocationContext::new("test", ".", false),
                },
            )
            .await
            .unwrap();

        assert_eq!(response.content, "hello from tool");
        assert!(!response.is_error);
    }
}
