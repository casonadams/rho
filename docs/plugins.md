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

MCP servers can be configured globally in `~/.config/rho/config.toml` or
per-project in `.rho/config.toml`:

```toml
[mcp]
enabled = true

[mcp.servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/Users/username/Desktop"]
enabled = true

[mcp.servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "ghp_..." }
enabled = true

[mcp.servers.playwright]
command = "npx"
args = ["-y", "@playwright/mcp", "--headless", "--isolated"]
enabled = true
```

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

## 3. Daemon Protocol (JSON-RPC 2.0 over Stdio)

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

## 4. Host Services API (`Plugin -> Host Requests`)

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

## 5. Building Plugins with `rho-plugin-sdk` (Rust)

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

## 6. Examples in Other Languages

Check [`examples/plugins/`](../examples/plugins/):

- **Python**:
  [`examples/plugins/python-guard/guard.py`](../examples/plugins/python-guard/guard.py)
- **Node.js**:
  [`examples/plugins/node-notifier/notifier.js`](../examples/plugins/node-notifier/notifier.js)
- **Rust**: [`examples/plugins/rust-guard/`](../examples/plugins/rust-guard/)
