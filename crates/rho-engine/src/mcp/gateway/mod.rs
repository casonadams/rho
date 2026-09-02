use super::client::{McpClient, McpToolDefinition};
use rig::tool::{DynamicTool, ToolOutput};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

#[cfg(test)]
mod tests;

#[derive(Debug, Deserialize)]
pub struct McpGatewayArgs {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub describe: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct McpBatchArgs {
    #[serde(default)]
    pub calls: Vec<McpSingleCall>,
}

#[derive(Debug, Deserialize)]
pub struct McpSingleCall {
    #[serde(default)]
    pub server: Option<String>,
    pub tool: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Clone)]
pub struct McpGateway {
    clients: BTreeMap<String, Arc<McpClient>>,
    tools: BTreeMap<String, (String, McpToolDefinition)>,
    max_output_bytes: usize,
}

impl McpGateway {
    pub fn new(
        clients: BTreeMap<String, Arc<McpClient>>,
        tool_defs: Vec<(String, McpToolDefinition)>,
        max_output_bytes: usize,
    ) -> Self {
        let mut tools = BTreeMap::new();
        for (server, def) in tool_defs {
            let key = format!("{}_{}", server, def.name);
            tools.insert(key, (server.clone(), def.clone()));
            tools.insert(def.name.clone(), (server, def));
        }
        Self {
            clients,
            tools,
            max_output_bytes,
        }
    }

    pub fn status(&self) -> Value {
        let mut servers = json!({});
        for (name, client) in &self.clients {
            let count = self.tools.values().filter(|(s, _)| s == name).count() / 2;
            servers[name] = json!({
                "server": client.server_name,
                "tool_count": count
            });
        }
        json!({
            "connected_servers": servers,
            "total_tools": self.tools.len() / 2
        })
    }

    pub fn search(&self, query: &str) -> Vec<Value> {
        let q = query.to_ascii_lowercase();
        let mut matched = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for (namespaced, (server, def)) in &self.tools {
            if namespaced.contains('_') && seen.insert(namespaced.clone()) {
                let desc = def.description.as_deref().unwrap_or("");
                if namespaced.to_ascii_lowercase().contains(&q) || desc.to_ascii_lowercase().contains(&q) {
                    matched.push(json!({
                        "server": server,
                        "name": def.name,
                        "namespaced_name": namespaced,
                        "description": desc,
                    }));
                }
            }
        }
        matched
    }

    pub fn describe(&self, tool_name: &str) -> Option<Value> {
        self.tools.get(tool_name).map(|(server, def)| {
            json!({
                "server": server,
                "name": def.name,
                "namespaced_name": format!("{server}_{}", def.name),
                "description": def.description,
                "inputSchema": def.input_schema,
            })
        })
    }

    pub async fn call(&self, call: McpSingleCall) -> Result<String, String> {
        let (server_name, original_tool) = if let Some(s) = call.server {
            (s, call.tool)
        } else if let Some((s, def)) = self.tools.get(&call.tool) {
            (s.clone(), def.name.clone())
        } else if let Some((s, rest)) = call.tool.split_once('_') {
            (s.to_string(), rest.to_string())
        } else {
            return Err(format!("Unknown tool: {}", call.tool));
        };

        let client = self
            .clients
            .get(&server_name)
            .ok_or_else(|| format!("Server '{server_name}' not connected"))?;

        let result = client
            .call_tool(&original_tool, call.args)
            .await
            .map_err(|e| format!("MCP Call failed: {e}"))?;

        let text = result.as_text_truncated(self.max_output_bytes);
        if result.is_error.unwrap_or(false) {
            Ok(format!("[Error] {text}"))
        } else {
            Ok(text)
        }
    }

    pub fn into_dynamic_tools(self) -> (DynamicTool, DynamicTool) {
        let gateway = Arc::new(self);

        let gateway_schema = json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "search", "describe", "call"],
                    "description": "Gateway action: 'status' (list servers), 'search' (find tools), 'describe' (tool schema), 'call' (invoke tool)"
                },
                "server": {
                    "type": "string",
                    "description": "Target server name (optional for search or when tool name is namespaced)"
                },
                "search": {
                    "type": "string",
                    "description": "Search query for discovering tools by name or description"
                },
                "describe": {
                    "type": "string",
                    "description": "Tool name to inspect input schema and parameter details"
                },
                "tool": {
                    "type": "string",
                    "description": "Tool name to execute"
                },
                "args": {
                    "type": "object",
                    "description": "Arguments to pass to the tool call"
                }
            }
        });

        let gw_clone = Arc::clone(&gateway);
        let gateway_tool = DynamicTool::new(
            "mcp",
            "MCP gateway — server status, tool search/describe, and single MCP tool calls. Use this to discover and invoke tools dynamically.",
            gateway_schema,
            move |_ctx, args| {
                let gw = Arc::clone(&gw_clone);
                Box::pin(async move {
                    let parsed = serde_json::from_value::<McpGatewayArgs>(args).unwrap_or(McpGatewayArgs {
                        action: None,
                        server: None,
                        search: None,
                        describe: None,
                        tool: None,
                        args: None,
                    });

                    if let Some(desc) = parsed.describe {
                        let out = match gw.describe(&desc) {
                            Some(info) => serde_json::to_string_pretty(&info).unwrap_or_default(),
                            None => format!("Tool '{desc}' not found"),
                        };
                        return Ok(ToolOutput::text(out));
                    }

                    if let Some(query) = parsed.search {
                        let results = gw.search(&query);
                        let out = serde_json::to_string_pretty(&results).unwrap_or_default();
                        return Ok(ToolOutput::text(out));
                    }

                    if let Some(tool) = parsed.tool {
                        let res = gw
                            .call(McpSingleCall {
                                server: parsed.server,
                                tool,
                                args: parsed.args.unwrap_or(Value::Null),
                            })
                            .await;
                        return match res {
                            Ok(text) => Ok(ToolOutput::text(text)),
                            Err(e) => Ok(ToolOutput::text(format!("[MCP Gateway Error] {e}"))),
                        };
                    }

                    if let Some(action) = parsed.action.as_deref()
                        && (action == "status" || action == "list")
                    {
                        let status = gw.status();
                        return Ok(ToolOutput::text(
                            serde_json::to_string_pretty(&status).unwrap_or_default(),
                        ));
                    }

                    let status = gw.status();
                    Ok(ToolOutput::text(
                        serde_json::to_string_pretty(&status).unwrap_or_default(),
                    ))
                })
            },
        );

        let script_schema = json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "server": { "type": "string" },
                            "tool": { "type": "string" },
                            "args": { "type": "object" }
                        },
                        "required": ["tool"]
                    },
                    "description": "Ordered list of MCP tool calls to execute sequentially"
                }
            },
            "required": ["calls"]
        });

        let gw_script = Arc::clone(&gateway);
        let script_tool = DynamicTool::new(
            "mcpScript",
            "Run multiple MCP tool calls in one request — batch execution across any connected MCP server.",
            script_schema,
            move |_ctx, args| {
                let gw = Arc::clone(&gw_script);
                Box::pin(async move {
                    let parsed =
                        serde_json::from_value::<McpBatchArgs>(args).unwrap_or(McpBatchArgs { calls: Vec::new() });
                    if parsed.calls.is_empty() {
                        return Ok(ToolOutput::text("No calls provided in batch"));
                    }

                    let mut outputs = Vec::new();
                    for (i, call) in parsed.calls.into_iter().enumerate() {
                        let call_idx = i + 1;
                        let tool_label = call.tool.clone();
                        let result = gw.call(call).await;
                        match result {
                            Ok(text) => {
                                outputs.push(format!("[Call {call_idx}: {tool_label}]\n{text}"));
                            }
                            Err(e) => {
                                outputs.push(format!("[Call {call_idx}: {tool_label} Failed]\n{e}"));
                                break;
                            }
                        }
                    }
                    Ok(ToolOutput::text(outputs.join("\n\n")))
                })
            },
        );

        (gateway_tool, script_tool)
    }
}
