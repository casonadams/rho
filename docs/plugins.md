# Model Context Protocol (MCP) & Extensions

`rho` extends its built-in tool suite (`read`, `write`, `edit`, `bash`, `search`, `fetch`) via the standard **Model Context Protocol (MCP)** and **plugin tool hooks**.

A typical extension setup combines both in `config.toml`:

```toml
[mcp]
enabled = true

[mcp.servers.playwright]
enabled = true
command = "npx"
args = [
  "-y",
  "@playwright/mcp",
  "--headless",
  "--isolated",
]

[plugins.permission]
enabled = true
path = "/Users/you/.config/rho/plugins/rho-plugin-permission"
```

## Configuring MCP Servers

MCP servers can be configured globally in `~/.config/rho/config.toml` or per-project in `.rho/config.toml`:

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

[mcp.servers.local_script]
command = "python3"
args = ["./tools/mcp_server.py"]
enabled = true
```

## Tool Namespacing

Every tool exposed by an MCP server is automatically namespaced using the server's configuration key:

`[mcp.servers.<server_name>]` + tool `foo` $\rightarrow$ model-facing tool `<server_name>_foo`

For example:
- Server `github` with tool `create_issue` $\rightarrow$ `github_create_issue`
- Server `filesystem` with tool `read_file` $\rightarrow$ `filesystem_read_file`

## Skills versus MCP Plugins

- **Skills**: Declarative markdown workflows (`SKILL.md`) that guide the model on specialized multi-step tasks (e.g., creating specs, reviewing code, planning implementations). They run inside the model prompt context and require no external binaries.
- **MCP Servers**: Standard JSON-RPC stdio processes that give the agent direct access to external capabilities, databases, APIs, browser automation, and custom tooling.

## Plugins (Tool Hooks)

Plugins are small binaries that hook every tool call. Before each tool runs,
the plugin receives the call on stdin and can **allow**, **deny** (with a
reason sent back to the model), or force an interactive approval prompt.

Configure plugins in `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[plugins.permission]
enabled = true
command = "rho-plugin-permission"        # looked up on PATH
# or point at a binary / plugin checkout directly:
# path = "/Users/you/.config/rho/plugins/rho-plugin-permission"
args = []
```

Path resolution rules:

- Relative `path` values resolve against the **project working directory**
  (where `rho` runs), not the config file's directory.
- `~` is **not** expanded in `path` — use absolute paths or `command`.
- A `path` may also be a plugin repo checkout: `rho` then looks for
  `<path>/target/release/<name>` and `<path>/target/debug/<name>` where
  `<name>` is the path's final component.

### Hook protocol

For each tool call the plugin process is spawned once and receives a single
JSON line on stdin:

```json
{"event":"pre_tool_call","tool":"bash","arguments":{"command":"git status"}}
```

After a tool finishes, `post_tool_result` fires with the same shape plus
`output` and `is_error`. The plugin's stdout is parsed as JSON:

- `{"action":"deny","reason":"..."}` (or `"block"`) — tool call is skipped and
  the reason is sent to the model.
- `{"action":"ask"}` (or `"prompt"`) — rho shows an interactive approval
  prompt; the user can Allow, Always allow, or Deny.
- Anything else (or empty stdout) — allowed.
- A non-zero exit code denies with stderr/stdout as the reason.
- `post_tool_result` responses are observational only; a missing or
  unspawnable binary fails open (allows).

### Example: permission plugin

[rho-plugin-permission](https://github.com/casonadams/rho-plugin-permission)
enforces allow/deny rules from `~/.config/rho/permission.toml` and prompts
interactively for everything else:

```toml
[allow]
bash = ["git *", "cargo *", "npm run *"]
edit = ["*"]

[deny]
bash = ["rm -rf *", "git push --force *"]
```

Install:

```sh
rho plugin install rho-plugin-permission
```

This runs `cargo install` (crates.io, or pass a git URL to build from source:
`rho plugin install https://github.com/casonadams/rho-plugin-permission`), then
registers `[plugins.rho-plugin-permission]` with `command =
"rho-plugin-permission"` in your `config.toml`. `rho plugin remove
rho-plugin-permission` reverses it (config entry plus `cargo uninstall`).

Alternatively, download the binary from the
[releases page](https://github.com/casonadams/rho-plugin-permission/releases),
extract it to `~/.config/rho/plugins/`, and add the plugin to `config.toml`
with an absolute `path` (or add `~/.config/rho/plugins` to your `PATH` and use
`command`).

See the
[plugin README](https://github.com/casonadams/rho-plugin-permission#readme)
for rule syntax and the full permission workflow.
