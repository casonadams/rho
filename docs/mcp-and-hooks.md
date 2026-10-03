# Model Context Protocol (MCP) & Lifecycle Hooks

`rho` extends its capabilities through two clean, standard systems:

1. **Model Context Protocol (MCP) Servers**: Standard out-of-process tool
   providers that expose external APIs, databases, browser automation, and
   custom tools.
2. **Lifecycle Hooks**: Lightweight, one-shot process hooks (`.agents/hooks/`) for
   security guardrails, request inspection, argument rewriting, and turn
   control.

---

## 1. Configuring MCP Servers

MCP servers are configured in standard JSON files:

- **Global**: `~/.config/mcp/mcp.json` (canonical, fallback: `~/.agents/mcp.json` or `~/.config/rho/mcp.json`)
- **Local / Workspace**: `.agents/mcp.json` (fallback: `.mcp.json`)

Local server definitions extend and override global servers on name collisions.

### Standard `.mcp.json` Example

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"],
      "lifecycle": "lazy",
      "idleTimeout": 600,
      "directTools": ["read_file", "list_directory"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "${GITHUB_TOKEN}"
      },
      "searchKeywords": {
        "search_code": ["grep", "find_code"]
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

### Server Lifecycles & Lazy Connections

MCP servers default to lazy on-demand lifecycle (`lifecycle: "lazy"`):
- No child processes are spawned on engine startup if metadata is cached in `~/.config/rho/mcp-cache.json`.
- When an MCP tool is invoked, the server process spawns transparently.
- Idle stdio server processes are automatically reaped after `idleTimeout` seconds of inactivity (default: 600s / 10m).
- Supported lifecycles: `"lazy"` (reaped when idle), `"eager"` (connected at startup), `"keep-alive"` (persistent, never reaped), and `"lazy-keep-alive"` (connected on first call, never reaped).

### Fine-Grained Tool Exposure & Discovery

- `directTools`: Controls whether tools appear directly in the model's active tool schema or are deferred behind `tool_search`. Can be `true` (all direct), `false` (all deferred), a list of tool names `["read_file"]`, or `"search"`.
- `includeTools` / `excludeTools`: Glob patterns to include or exclude specific tools from the server.
- `searchKeywords`: Per-tool keyword aliases (e.g. `{"search_code": ["grep"]}`) boosting relevancy during `tool_search`.

### Project Server Trust

To guard against malicious repositories, project-scoped MCP servers (`.mcp.json`) require authorization based on the SHA-256 hash of their execution command, arguments, and environment variables. Approved hashes are stored in `~/.config/rho/mcp_approved_servers.json`.

### Managing MCP Servers via CLI

You can manage configured servers with `rho mcp`:

```sh
# List active servers
rho mcp list

# Test connection and tool discovery
rho mcp test filesystem

# Log in to an authenticated remote MCP server
rho mcp login remote_jira

# Add an MCP server to global config (~/.agents/mcp.json)
rho mcp add db "https://mcp.db.example.com/mcp"

# Remove an MCP server
rho mcp remove db
```

Or view and test active servers interactively inside the REPL with `/mcp`.

### Deferred Tool Search

When configured MCP servers expose more tools than the deferral threshold (`defer_threshold`, default: 4), `rho` defers individual tool definitions behind a compact `tool_search` tool to conserve context tokens. When the model needs a tool, it calls `tool_search` with keywords or tool names, which dynamically loads the matching tools with their full JSON schemas for subsequent turns.

To avoid extra turn latency when discovering and running a tool, `tool_search` also supports an optional `execute` object (`tool`, `arguments`) to search, activate, and execute a tool immediately in the same turn. Tools already invoked in conversation history are automatically preserved and pre-activated upon session resumption.

---

## 2. Agent Lifecycle Hooks

Lifecycle hooks allow developers and security teams to intercept agent turns and
tool executions without building complex daemon runtimes.

Hooks are executable scripts or binaries placed in `<workspace>/.agents/hooks/` (or user fallback `~/.agents/hooks/`):

| Hook Script            | Trigger Point                                                                              |
| :--------------------- | :----------------------------------------------------------------------------------------- |
| `on_tool_call`         | Runs before a tool executes. Can allow, stop, skip, rewrite arguments, or prompt the user. |
| `on_tool_result`       | Runs after a tool executes. Can inspect or rewrite tool output before the model sees it.   |
| `on_completion_call`   | Runs before prompt submission to the LLM provider.                                         |
| `on_invalid_tool_call` | Runs when an LLM requests a non-existent tool. Can retry with feedback or halt.            |
| `turn_start`           | Notifies turn start with the user's prompt.                                                |
| `turn_end`             | Notifies turn completion.                                                                  |

### How Hooks Work

1. When an event fires, `rho` runs the hook script as an isolated child process
   with a 5-second timeout.
2. `rho` pipes event details as single-line JSON to `stdin` and reads the
   decision JSON from `stdout`.
3. The hook process exits immediately; no persistent daemon runs in the
   background.
4. If a hook times out or exits non-zero without a valid action, execution fails
   closed for safety.

### Hook Decision Protocol (`stdout`)

A hook outputs one of the following JSON decisions:

- **Continue**: `{"action": "continue"}` (or empty output on exit 0).
- **Stop**: `{"action": "stop", "reason": "blocked by rule"}` (immediately halts
  the turn).
- **Skip**: `{"action": "skip", "reason": "tool skipped"}` (skips tool
  execution).
- **Rewrite Arguments**:
  `{"action": "rewrite_args", "args": {"safe_arg": true}}`.
- **Rewrite Result**:
  `{"action": "rewrite_result", "result": "sanitized output"}`.
- **Ask User**: `{"action": "ask", "message": "Allow execution?"}` (triggers
  rho's native TUI modal).

### Example Recipes

Ready-to-use recipes are provided in `examples/hooks/`:

- **[`rtk-rewrite.py`](../examples/hooks/rtk-rewrite.py)**: Intercepts `on_tool_call` to automatically optimize bash commands using [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk), compressing CLI output before it enters LLM context.
- **[`guard-bash.sh`](../examples/hooks/guard-bash.sh)**: Blocks destructive shell commands (`rm -rf`, `git reset --hard`) before execution.
- **[`python-audit.py`](../examples/hooks/python-audit.py)**: Audits tool call events to a local file.

