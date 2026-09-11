# Model Context Protocol (MCP) & Plugins

`rho` extends its capabilities through two systems:

1. **Model Context Protocol (MCP) Servers**: Out-of-process JSON-RPC tool
   providers that expose external APIs, databases, browser automation, and
   custom tools.
2. **Rig-Native Plugin Subsystem**: Event hooks, dynamic tools, custom
   providers, and host UI integration for security guardrails, request steering,
   and tool transformation.

---

## 1. Configuring MCP Servers

MCP servers can be configured globally in `~/.config/rho/config.toml`,
per-project in `.rho/config.toml`, or via standard `.mcp.json` at the workspace root:

```toml
[mcp]
enabled = true

# Local stdio subprocess
[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/Users/username/Desktop"]
mode = "direct" # "direct" | "gateway" | "auto"
enabled = true

# Remote Streamable HTTP endpoint with OAuth or Bearer token
[mcp.servers.remote_jira]
url = "https://mcp.atlassian.example.com/mcp"
transport = "streamable-http" # "streamable-http" | "sse"
headers = { Authorization = "Bearer env:JIRA_API_TOKEN" }
mode = "gateway"
include_tools = ["search_issues", "create_issue"]
enabled = true
```

### Standard `.mcp.json` Compatibility

`rho` transparently reads standard workspace `.mcp.json` files:

```json
{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@playwright/mcp", "--headless"],
      "env": { "HEADLESS": "true" }
    },
    "cloud": {
      "url": "https://mcp.example.com/mcp",
      "transport": "streamable-http"
    }
  }
}
```

### Tool Exposure & Context Optimization

- `mode = "direct"`: Tools are exposed directly in the agent's active toolset.
- `mode = "gateway"`: Tools are searched and called dynamically via the `mcp` gateway tool to conserve token budget.
- `mode = "auto"` (default): Uses direct exposure if tool count $\le$ 5, gateway if $> 5$.
- `include_tools` & `exclude_tools`: Allowlist or denylist specific tool names.

### MCP Management CLI & Interactive REPL

Manage MCP servers directly from the terminal:

```bash
# List configured servers and statuses
rho mcp list

# Test connection, handshake latency, tools, resources, and prompts
rho mcp test filesystem

# Authenticate with a remote OAuth 2.1 MCP server
rho mcp login remote_jira

# Add or remove servers
rho mcp add db "https://mcp.db.internal/mcp"
rho mcp remove db
```

In the interactive REPL, press `/mcp` to open the clean management modal to toggle servers on and off dynamically.

### Tool Namespacing

Every tool exposed by an MCP server is automatically namespaced using the
server's configuration key: `[mcp.servers.<server_name>]` + tool `foo`
$\rightarrow$ model-facing tool `<server_name>_foo`

---

## 2. Plugins (Rig-Native Hook Subsystem)

Plugins in `rho` are long-running daemon processes or native Rust plugins that
hook into [Rig's agent lifecycle](https://rig.rs/docs/concepts/hooks) to
observe, steer, or augment execution.

### Configuring Plugins

Configure plugins in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[plugins.permission]
enabled = true
command = "rho-plugin-permission" # Looked up on PATH
# or point at a binary / checkout directly:
# path = "/Users/you/.config/rho/plugins/rho-plugin-permission"
args = []
```

#### Path Resolution Rules:

- Relative `path` values resolve against the **working directory** where `rho`
  runs.
- `~` is not expanded in `path` — use absolute paths or `command`.
- A `path` may point to a cargo project: `rho` automatically resolves
  `<path>/target/release/<name>` or `<path>/target/debug/<name>`.

---

## 3. Plugin Package & Artifact Management

`rho` provides automated package management to install, inspect, update, and
remove plugin binaries directly from GitHub Releases without requiring a local
Rust toolchain or `cargo` CLI:

### A. Installing Plugins

Install prebuilt release binaries into `~/.cargo/bin` and register them in
`~/.config/rho/config.toml`:

```bash
# Bare plugin name (resolves to casonadams/rho-plugin-<name>)
rho install permission

# Pinned release version or tag
rho install permission@v0.3.0

# GitHub repository slug
rho install casonadams/rho-plugin-permission

# Full HTTPS GitHub URL
rho install https://github.com/casonadams/rho-plugin-permission

# Overwrite existing configuration or binary
rho install permission --force
```

_(Visible alias: `rho plugin install <target>`)_

### B. Listing & Inspecting Plugins

Audit all configured plugins, resolved binary locations, artifact health
(`Installed (active)` vs `Missing`), and management origin:

```bash
rho plugin ls
# or:
rho plugin list
```

### C. Updating Plugins & Self-Update

Keep `rho` and installed plugins up to date with precompiled releases:

```bash
# Self-update rho to the latest GitHub release
rho update

# Update all configured plugins
rho update all

# Update a specific plugin
rho update permission
```

_(Visible alias: `rho plugin update [target]`)_

### D. Removing Plugins

Remove plugin configuration and safely delete the executable from
`~/.cargo/bin`:

```bash
# Remove plugin and delete ~/.cargo/bin binary
rho remove permission

# Remove plugin from config.toml but retain the binary on disk
rho remove permission --keep-binary
```

_(Visible aliases: `rho uninstall <name>`, `rho plugin remove <name>`,
`rho plugin rm <name>`)_

> **Safety boundary**: Binary deletion is strictly confined to
> `~/.cargo/bin/<executable>`. Binaries located outside this directory (such as
> system tools or local scripts) are never unlinked.

---

## 4. Daemon Protocol (JSON-RPC 2.0 over Stdio)

External plugins run as persistent processes communicating via standard JSON-RPC
2.0 over standard I/O (stdin/stdout).

### A. Lifecycle & Hook Events (`Host -> Plugin`)

The engine dispatches Rig lifecycle events to active plugins:

| Method                     | Event Payload                                                                                     | Description                                                   |
| :------------------------- | :------------------------------------------------------------------------------------------------ | :------------------------------------------------------------ |
| `hook/tool_call`           | `{"event": "tool_call", "tool_name": "...", "args": {...}}`                                       | Intercept tool call before execution.                         |
| `hook/tool_result`         | `{"event": "tool_result", "tool_name": "...", "args": {...}, "output": "...", "is_error": false}` | Inspect output after tool execution.                          |
| `hook/invalid_tool_call`   | `{"event": "invalid_tool_call", "tool_name": "...", "args": {...}, "available_tools": [...]}`     | Intercept unknown / hallucinated tool calls for self-healing. |
| `hook/completion_call`     | `{"event": "completion_call", "turn": 1, "prompt": {...}, "history": [...]}`                      | Inspect or patch turn request parameters.                     |
| `hook/completion_response` | `{"event": "completion_response", "prompt": {...}, "response": [...]}`                            | Audit raw completion output and tokens.                       |

### B. Steering Actions (`Plugin -> Host Response`)

In response to any hook request, the plugin returns a standard Rig `Flow`
action:

- `{"action": "continue"}` — Proceed normally.
- `{"action": "skip", "reason": "..."}` — Skip tool execution and return
  `reason` as the tool result.
- `{"action": "rewrite_args", "args": {...}}` — Run the tool with replacement
  JSON arguments.
- `{"action": "rewrite_result", "result": "..."}` — Replace the output string
  returned to the model.
- `{"action": "override_request", "request": {"temperature": 0.0, "active_tools": ["bash"]}}`
  — Patch turn parameters.
- `{"action": "repair", "tool_name": "bash"}` — Repair an invalid/aliased tool
  name on the fly.
- `{"action": "retry", "feedback": "..."}` — Send error feedback back to the LLM
  to self-correct.
- `{"action": "terminate", "reason": "..."}` — Stop the agent turn immediately.

---

## 5. Host Services API (`Plugin -> Host Requests`)

While evaluating an event, a plugin can request host services (such as UI
modals) via bidirectional JSON-RPC:

### 1. `host/ui/confirm`

Presents a Yes/No modal in `rho`'s terminal UI:

```json
{
  "jsonrpc": "2.0",
  "id": 100,
  "method": "host/ui/confirm",
  "params": {
    "title": "Dangerous Command",
    "message": "Allow 'rm -rf target'?",
    "default_yes": false
  }
}
```

Host response:

```json
{ "jsonrpc": "2.0", "id": 100, "result": { "confirmed": true } }
```

### 2. `host/ui/select`

Presents a selectable list of options with preview descriptions:

```json
{
  "jsonrpc": "2.0",
  "id": 101,
  "method": "host/ui/select",
  "params": {
    "title": "Choose Target",
    "options": [
      { "label": "Development", "description": "Local dev cluster" },
      { "label": "Production", "description": "Live production database" }
    ],
    "allow_custom": true
  }
}
```

Host response:

```json
{ "jsonrpc": "2.0", "id": 101, "result": { "selected": 0, "cancelled": false } }
```

### 3. `host/ui/notify`

Emits a notice into the terminal transcript:

```json
{
  "jsonrpc": "2.0",
  "id": 102,
  "method": "host/ui/notify",
  "params": {
    "message": "Quota usage: 85%",
    "level": "warning"
  }
}
```

_Note: In headless/non-interactive mode (`has_ui == false`), confirmation and
input requests fail closed (`confirmed: false`, `cancelled: true`)
automatically._

---

## 6. Building Plugins with `rho-plugin-sdk` (Rust)

For Rust developers, the official
[`rho-plugin-sdk`](https://crates.io/crates/rho-plugin-sdk) eliminates all
protocol boilerplate:

```toml
[dependencies]
rho-plugin-sdk = "0.1.3"
async-trait = "0.1"
tokio = { version = "1.43", features = ["macros", "rt-multi-thread"] }
```

```rust
use async_trait::async_trait;
use rho_plugin_sdk::{Flow, HostContext, Plugin, StepEvent, serve};

struct MyGuard;

#[async_trait]
impl Plugin for MyGuard {
    fn name(&self) -> &str {
        "my-guard"
    }

    async fn on_event(&self, event: StepEvent, ctx: &HostContext) -> Flow {
        match event {
            StepEvent::ToolCall { tool_name, args } => {
                if tool_name == "bash" && args.get("command").unwrap().contains("sudo") {
                    let ok = ctx.confirm("Security Gate", "Allow sudo?").await;
                    if !ok {
                        return Flow::skip("Permission denied by user. Do not retry.");
                    }
                }
                Flow::cont()
            }
            _ => Flow::cont(),
        }
    }
}

#[tokio::main]
async fn main() {
    serve(MyGuard).await;
}
```

---

## 7. Examples in Other Languages

Check [`examples/plugins/`](../examples/plugins/):

- **Python**:
  [`examples/plugins/python-guard/guard.py`](../examples/plugins/python-guard/guard.py)
- **Node.js**:
  [`examples/plugins/node-notifier/notifier.js`](../examples/plugins/node-notifier/notifier.js)
- **Rust**: [`examples/plugins/rust-guard/`](../examples/plugins/rust-guard/)
