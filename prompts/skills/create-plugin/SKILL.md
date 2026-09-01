---
name: create-plugin
description: Create, test, and package an MCP (Model Context Protocol) tool server or extension for rho. Use when asked to write an extension or tool plugin for rho.
argument-hint: "<plugin-idea-or-specification>"
---

# Creating an MCP Tool Plugin for `rho`

`rho` extensions are standard Model Context Protocol (MCP) servers communicating over JSON-RPC on standard I/O.

## 1. Defining Tools

An MCP server announces its tools via the `tools/list` endpoint and handles calls via `tools/call`.

### MCP Server Example (Node / Python / Rust / Bash)

A simple JSON-RPC MCP server responds to `initialize`, `tools/list`, and `tools/call`:

```json
// tools/list response:
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "my_tool",
        "description": "Custom extension tool for rho",
        "inputSchema": {
          "type": "object",
          "properties": {
            "query": { "type": "string" }
          },
          "required": ["query"]
        }
      }
    ]
  }
}
```

```json
// tools/call response:
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "Execution result content here"
      }
    ],
    "isError": false
  }
}
```

## 2. Configuration

Add the server to `~/.config/rho/config.toml` or workspace `.rho/config.toml`:

```toml
[mcp]
enabled = true

[mcp.servers.my_plugin]
command = "node"
args = ["./path/to/server.js"]
enabled = true
```

## 3. Tool Namespacing

Tools are automatically namespaced as `<server_name>_<tool_name>` (e.g. `my_plugin_my_tool`) and exposed directly to the model as standard tools.
