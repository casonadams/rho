use super::client::McpToolDefinition;
use crate::plugin::capability::{CapabilityId, CapabilityKind};
use crate::plugin::contract::{ExecutionMode, OperationEffect, ToolDescriptor};

pub fn mcp_tool_to_descriptor(server_name: &str, tool: &McpToolDefinition) -> ToolDescriptor {
    let capability_id = CapabilityId::new(CapabilityKind::Tool, &tool.name).unwrap_or_else(|_| {
        let clean_name = tool.name.replace([':', '-'], "_");
        CapabilityId::new(CapabilityKind::Tool, clean_name)
            .unwrap_or_else(|_| format!("tool:{}", tool.name.replace(' ', "_")).parse().unwrap())
    });

    let description = tool.description.as_deref().unwrap_or("External MCP tool").to_string();

    let mut schema = tool.input_schema.clone();
    if !schema.is_object() {
        schema = serde_json::json!({
            "type": "object",
            "properties": {}
        });
    }

    ToolDescriptor {
        id: capability_id,
        description,
        argument_schema: schema,
        prompt_guidance: format!("MCP tool provided by server '{server_name}'."),
        effects: vec![OperationEffect::ExecuteProcess],
        execution_mode: ExecutionMode::Sequential,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_to_descriptor() {
        let tool = McpToolDefinition {
            name: "get_issue".to_string(),
            description: Some("Fetch a Linear issue".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"}
                },
                "required": ["id"]
            }),
        };

        let descriptor = mcp_tool_to_descriptor("linear", &tool);
        assert_eq!(descriptor.id.name(), "get_issue");
        assert_eq!(descriptor.description, "Fetch a Linear issue");
        assert_eq!(descriptor.execution_mode, ExecutionMode::Sequential);
    }
}
