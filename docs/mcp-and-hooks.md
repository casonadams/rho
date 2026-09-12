# Model Context Protocol (MCP) & Lifecycle Hooks

`rho` extends its capabilities through two clean, standard systems:

1. **Model Context Protocol (MCP) Servers**: Standard out-of-process tool providers that expose external APIs, databases, browser automation, and custom tools.
2. **Lifecycle Hooks**: Lightweight, one-shot process hooks (`.rho/hooks/`) for security guardrails, request inspection, argument rewriting, and turn control.

---

## 1. Configuring MCP Servers

MCP servers are configured in standard JSON files:

- **Global**: `~/.agents/mcp.json` (fallback: `~/.config/rho/mcp.json`)
- **Local / Workspace**: `.mcp.json` (fallback: `.rho/mcp.json`)

Local server definitions extend and override global servers on name collisions.

### Standard `.mcp.json` Example

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "${GITHUB_TOKEN}"
      }
    },
    "remote_jira": {
      "url": "https://mcp.atlassian.example.com/mcp",
      "transport": "streamable-http",
      "headers": {
        "Authorization": "Bearer ${JIRA_API_TOKEN}"
      }
    }
  }
}
```

### Managing MCP Servers via CLI

You can manage configured servers with `rho mcp`:

```sh
# List active servers
rho mcp list

# Test connection and tool discovery
rho mcp test filesystem

# Add an MCP server to global config (~/.agents/mcp.json)
rho mcp add db "https://mcp.db.example.com/mcp"

# Remove an MCP server
rho mcp remove db
```

Or view and test active servers interactively inside the REPL with `/mcp`.

---

## 2. Agent Lifecycle Hooks

Lifecycle hooks allow developers and security teams to intercept agent turns and tool executions without building complex daemon runtimes.

Hooks are executable scripts or binaries placed in `<workspace>/.rho/hooks/`:

| Hook Script | Trigger Point |
| :--- | :--- |
| `on_tool_call` | Runs before a tool executes. Can allow, stop, skip, rewrite arguments, or prompt the user. |
| `on_tool_result` | Runs after a tool executes. Can inspect or rewrite tool output before the model sees it. |
| `on_completion_call`| Runs before prompt submission to the LLM provider. |
| `on_invalid_tool_call` | Runs when an LLM requests a non-existent tool. Can retry with feedback or halt. |
| `turn_start` | Notifies turn start with the user's prompt. |
| `turn_end` | Notifies turn completion. |

### How Hooks Work

1. When an event fires, `rho` runs the hook script as an isolated child process with a 5-second timeout.
2. `rho` pipes event details as single-line JSON to `stdin` and reads the decision JSON from `stdout`.
3. The hook process exits immediately; no persistent daemon runs in the background.
4. If a hook times out or exits non-zero without a valid action, execution fails closed for safety.

### Hook Decision Protocol (`stdout`)

A hook outputs one of the following JSON decisions:

- **Continue**: `{"action": "continue"}` (or empty output on exit 0).
- **Stop**: `{"action": "stop", "reason": "blocked by rule"}` (immediately halts the turn).
- **Skip**: `{"action": "skip", "reason": "tool skipped"}` (skips tool execution).
- **Rewrite Arguments**: `{"action": "rewrite_args", "args": {"safe_arg": true}}`.
- **Rewrite Result**: `{"action": "rewrite_result", "result": "sanitized output"}`.
- **Ask User**: `{"action": "ask", "message": "Allow execution?"}` (triggers rho's native TUI modal).
