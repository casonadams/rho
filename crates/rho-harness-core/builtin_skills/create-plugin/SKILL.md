---
name: create-plugin
description: Create, test, and package an MCP tool server or Rig-native lifecycle hook plugin for rho. Use when asked to write an extension, tool, or hook plugin for rho.
argument-hint: "<plugin-idea-or-specification>"
---

# Creating Plugins for `rho`

`rho` supports two extension mechanisms:
1. **MCP Tool Servers** (`[mcp.servers.<name>]`): External tools exposed to the model via JSON-RPC `tools/list` and `tools/call`.
2. **Rig-Native Hook Plugins** (`[plugins.<name>]`): Long-running daemon processes or native Rust plugins that hook into the agent lifecycle to observe, approve, rewrite, or self-heal tool calls and turn requests.

---

## 1. Creating a Rig-Native Hook Plugin

A hook plugin receives Rig lifecycle events over standard I/O and returns steering actions. It can also request interactive confirmation modals from `rho`'s TUI via Host Services.

### A. Lifecycle Events (`Host -> Plugin`)
* `hook/tool_call` — Intercept tool before execution.
* `hook/tool_result` — Observe or sanitize output after tool execution.
* `hook/invalid_tool_call` — Self-heal hallucinated or aliased tool names.
* `hook/completion_call` — Patch temperature, active tools, or system prompts per turn.
* `hook/completion_response` — Audit raw completion telemetry and tokens.

### B. Steering Actions (`Plugin -> Host`)
* `{"action": "continue"}` — Allow operation.
* `{"action": "skip", "reason": "..."}` — Skip execution and send reason as tool result.
* `{"action": "rewrite_args", "args": {...}}` — Replace tool execution arguments.
* `{"action": "rewrite_result", "result": "..."}` — Sanitize/redact tool output.
* `{"action": "repair", "tool_name": "bash"}` — Repair an invalid tool name.
* `{"action": "retry", "feedback": "..."}` — Feed back error to the LLM to retry.
* `{"action": "terminate", "reason": "..."}` — Abort agent turn.

### C. Host UI Services (`Plugin -> Host`)
* `host/ui/confirm` — Show Yes/No modal (`{"title": "...", "message": "..."}`).
* `host/ui/select` — Show selection list (`{"title": "...", "options": [...]}`).
* `host/ui/notify` — Emit notification (`{"message": "...", "level": "info"}`).

### Example: Permission & Safety Hook Plugin (Bash / Python / Rust)
```sh
#!/bin/sh
# Example daemon reading JSON-RPC requests from stdin
while IFS= read -r line; do
  case "$line" in
    *"hook/tool_call"*)
      # Match dangerous patterns or ask user via Host UI
      echo '{"jsonrpc":"2.0","id":100,"method":"host/ui/confirm","params":{"title":"Dangerous Action","message":"Allow tool execution?"}}'
      ;;
    *"\"confirmed\":true"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"continue"}}'
      ;;
    *"\"confirmed\":false"*)
      echo '{"jsonrpc":"2.0","id":1,"result":{"action":"skip","reason":"Blocked by user"}}'
      ;;
  esac
done
```

Configure in `config.toml`:
```toml
[plugins.my_guard]
enabled = true
command = "my-guard-plugin" # On PATH or path = "/path/to/binary"
```

---

## 2. Creating an MCP Tool Server

An MCP server announces its tools via `tools/list` and handles calls via `tools/call`.

### MCP Server Example (Node.js / Python)

```json
// tools/list response:
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "lookup",
        "description": "Custom database lookup tool",
        "inputSchema": {
          "type": "object",
          "properties": { "id": { "type": "string" } },
          "required": ["id"]
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
    "content": [{ "type": "text", "text": "Record details" }],
    "isError": false
  }
}
```

Configure in `config.toml`:
```toml
[mcp]
enabled = true

[mcp.servers.my_db]
command = "python3"
args = ["./tools/server.py"]
enabled = true
```

Tools are exposed to the model as `<server_name>_<tool_name>` (e.g. `my_db_lookup`).
