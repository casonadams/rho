# Model Context Protocol (MCP) & Extensions

`rho` extends its built-in tool suite (`read`, `write`, `edit`, `bash`, `search`, `fetch`) via the standard **Model Context Protocol (MCP)**.

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
