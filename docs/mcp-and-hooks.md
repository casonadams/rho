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

- **Global**: the first existing file of `~/.config/mcp/mcp.json`, `~/.agents/mcp.json`, `~/.claude/mcp.json`, `~/.claude.json`, then `~/.config/rho/mcp.json`. When `RHO_HOME` is set, it replaces `~` for the first four paths (for example `$RHO_HOME/.claude.json`) and the real home directory is not read; `$RHO_HOME/mcp.json` is the last candidate.
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
    },
    "github-hosted": {
      "url": "https://api.githubcopilot.com/mcp/x/all",
      "headers": {
        "Authorization": "!printf 'Bearer %s' \"$(gh auth token)\""
      }
    }
  }
}
```

Header values expand `${VAR}` and `env:VAR`. A value starting with `!` runs as a shell command when the server connects, and its trimmed stdout becomes the header value; a failing or empty command fails the connection.

### Server Lifecycles & Lazy Connections

MCP servers default to lazy on-demand lifecycle (`lifecycle: "lazy"`):
- No child processes are spawned on engine startup if metadata is cached in `~/.config/rho/mcp-cache.json`. Cache entries older than 24 hours are still used at startup and refreshed in the background.
- Uncached servers are discovered in parallel. Startup waits at most `[mcp] startup_timeout_ms` from `config.toml` (default 3000; `0` waits for every server). Servers still connecting after that are reported once ("MCP still connecting: ...") and their tools join `tool_search` when they finish.
- A server that sends `notifications/tools/list_changed` has its tool list re-read and the cache updated. Over HTTP rho sees these notifications only inside streamed (SSE) responses to its own requests, because it does not open the optional standalone GET stream.
- When an MCP tool is invoked, the server process spawns transparently.
- Idle stdio server processes are automatically reaped after `idleTimeout` seconds of inactivity (default: 600s / 10m).
- Supported lifecycles: `"lazy"` (reaped when idle), `"eager"` (connected at startup), `"keep-alive"` (persistent, never reaped), and `"lazy-keep-alive"` (connected on first call, never reaped).

### Timeouts, Cancellation, and Reconnects

- Tool calls may run for 600 seconds; other requests (`initialize`, listing tools, reading resources) time out after 30 seconds. A per-server `timeout` (seconds) replaces both.
- rho sends a `progressToken` with every tool call. Each `notifications/progress` the server sends starts a fresh timeout window, up to one hour in total.
- When a call times out or you press Esc, rho sends `notifications/cancelled` so the server can stop the work, over both stdio and HTTP.
- If a stdio server exits or an HTTP session expires (`404` for a request carrying `Mcp-Session-Id`), rho reconnects and retries the request once. A tool call that was already sent when the connection dropped is not retried, because the server may have run it; the call reports an error and the next call reconnects. After 5 reconnects within 30 seconds it stops reconnecting that server until `/reload` and reports why.
- Problems found after startup (a slow server that needs a sign-in, a refresh that fails, or a token that can no longer be refreshed mid-session) are shown as one warning when the REPL is idle; tool calls to such a server return an error that names the fix, such as `/mcp login <server>`.
- Server instructions are read when the engine is built. Instructions from servers that finish connecting later, or that change on refresh, take effect at the next `/reload` or login, so the system prompt (and the provider's prompt cache) stays fixed within a session.

### Fine-Grained Tool Exposure & Discovery

- `directTools`: Controls whether tools appear directly in the model's active tool schema or are deferred behind `tool_search`. Can be `true` (all direct), `false` (all deferred), a list of tool names `["read_file"]`, or `"search"`.
- `includeTools` / `excludeTools`: Glob patterns to include or exclude specific tools from the server.
- `searchKeywords`: Per-tool keyword aliases (e.g. `{"search_code": ["grep"]}`) boosting relevancy during `tool_search`.
- `instructions`: Set to `false` to leave out the usage instructions the server returns from `initialize`. Otherwise rho adds them to the system prompt in an `<mcp_server_instructions>` block, attributed to the server, marked as server-controlled, and capped at 4 KB per server.

Direct tools are named `<server>_<tool>`. Characters outside `[A-Za-z0-9_-]` become `_`, and names longer than 64 characters are shortened with a hash suffix. A direct tool whose name would collide with a built-in or another MCP tool stays behind `tool_search` and rho reports the conflict.

### Project Server Trust

To guard against malicious repositories, servers defined by the current directory's project file (`.agents/mcp.json`, `.claude/mcp.json`, or `.mcp.json`) are skipped until you approve them. Global servers never need approval, even when you run rho inside a project. An approval covers the SHA-256 hash of the server's command, arguments, environment variables, URL, headers, transport, OAuth settings, exposure mode, tool filters (`include_tools`, `exclude_tools`, `direct_tools`), and `instructions` for that workspace, so editing any of them requires approving it again. Approvals are stored in `~/.config/rho/mcp_approved_servers.json`.

```sh
# Show which project servers are approved
rho mcp list

# Approve one project server, or every project server in this directory
rho mcp approve playwright-test
rho mcp approve
```

Restart rho or run `/reload` to load newly approved servers. `RHO_TRUST_PROJECT=1` trusts every project server (and project hooks) without approval.

### Managing MCP Servers via CLI

You can manage configured servers with `rho mcp`:

```sh
# List active servers
rho mcp list

# Test connection and tool discovery
rho mcp test filesystem

# Add an MCP server to rho's global config (~/.config/rho/mcp.json)
rho mcp add db "https://mcp.db.example.com/mcp"

# Remove an MCP server
rho mcp remove db

# Sign in to (or out of) a remote server that uses OAuth
rho mcp login linear
rho mcp logout linear

# Approve this directory's project servers (see Project Server Trust)
rho mcp approve
```

Or view and test active servers interactively inside the REPL with `/mcp`, and
sign in with `/mcp login <server>` / `/mcp logout <server>`.

### Prompts and Resources

Server prompts are user-invoked: `/mcp prompts [server]` lists them with their
arguments (`*` marks required ones), and `/mcp prompt <server> <name>
[key=value ...]` fetches a prompt and sends its messages as your next prompt.
Argument values cannot contain spaces. Tab completes the subcommands and
server names.

The model reads server resources through the `mcp` tool: `{"server": "docs",
"action": "list_resources"}` lists them and `{"server": "docs", "action":
"read_resource", "uri": "..."}` returns their contents.

### Remote Servers with OAuth

Remote (URL) servers that require OAuth 2.1, per the MCP authorization spec,
work without any token in your config:

```json
{
  "mcpServers": {
    "linear": { "url": "https://mcp.linear.app/mcp" }
  }
}
```

Run `/mcp login linear` (or `rho mcp login linear`). rho reads the server's
`401` challenge and protected resource metadata, discovers its authorization
server, registers itself with dynamic client registration, and opens the
browser for a PKCE sign-in that names the server as the token's `resource`.
In the REPL the sign-in stays inside the TUI: progress and the authorization
URL appear in the transcript, and Esc cancels. You can also start it by
pressing Enter on a server marked as needing sign-in in the `/mcp` list.
rho refuses authorization server metadata whose `issuer` differs from the
issuer it asked for (a trailing slash is ignored), and when the requested
scopes include `offline_access` it adds `prompt=consent` unless
`authorizationParams` already sets `prompt`.
A remote server counts as OAuth when it has an `oauth` block or configures no
headers; a server with headers such as `x-api-key` uses those instead.
Tokens are stored in `~/.config/rho/mcp-auth.json` (mode `0600`), sent as
`Authorization: Bearer` on every request, refreshed before they expire, and
refreshed once and retried if the server rejects one. When servers need a
sign-in, rho shows one warning after startup, `/reload`, or a login that lists
them all, and the `/mcp` list marks them. Tab completes `/mcp login` and
`/mcp logout` subcommands and the names of OAuth-capable remote servers.

If the provider does not support dynamic registration, register an OAuth app
with it and add an `oauth` block. `redirectUri` must exactly match the
loopback redirect URI you registered (`http://localhost:<port>/<path>` or
`http://127.0.0.1:...`); `redirectPort` is shorthand for
`http://localhost:<port>/callback`. With a `clientId` and neither field, rho
uses `http://localhost:3000/callback`, the URI oh-my-pi registers, so a
shared `mcp.json` works in both tools:

```json
"github": {
  "url": "https://api.githubcopilot.com/mcp/",
  "oauth": {
    "clientId": "Iv1.abc123",
    "clientSecret": "${GITHUB_MCP_CLIENT_SECRET}",
    "scopes": ["repo", "read:org"],
    "redirectUri": "http://localhost:8765/callback"
  }
}
```

`scopes` (an array or a space-separated string; `scope` also works) overrides
the scopes the server asks for. `authorizationParams` adds extra query
parameters to the authorize URL without overriding protocol ones, for example
`{ "access_type": "offline", "prompt": "consent" }` so Google issues a
refresh token. A server with a static
`Authorization` header (like `remote_jira` above) keeps using that header and
never uses OAuth.

### Deferred Tool Search

When configured MCP servers expose more tools than the deferral threshold (`defer_threshold`, default: 4), `rho` keeps individual tool definitions out of the model's tool list to conserve context tokens. The model finds them with `tool_search`, which returns matching tools with their full input schemas, and calls them through the `mcp` tool with `{"server": ..., "tool": ..., "arguments": {...}}`. Calls through `mcp` go through the same permission checks as direct MCP tools (`mcp` rules such as `server:tool`).

Searching never changes the tool list sent to the provider, so the provider's prompt cache stays valid for the whole session. Tools already invoked directly in conversation history stay direct when a session resumes.

### Tool Results

MCP results keep their text, embedded resources (`[Resource: uri]` plus text), resource links, and `structuredContent` (appended as a JSON block unless a text block already carries it). The first image is passed to models that accept tool-result images; other images and audio are summarized. Output larger than `output_max_bytes` is truncated and the full text is saved under `.rho/artifacts/`, with the path noted in the result.

---

## 2. Agent Lifecycle Hooks

Lifecycle hooks allow developers and security teams to intercept agent turns and
tool executions without building complex daemon runtimes.

Hooks are executable scripts or binaries placed in `~/.agents/hooks/` or `<workspace>/.agents/hooks/`. Hook directories
given with `--hook-dir` or `RHO_HOOK_DIR` and user hooks run before project hooks. Project hooks run only when the
workspace is trusted with `RHO_TRUST_PROJECT=1`:

| Hook Script            | Trigger Point                                                                              |
| :--------------------- | :----------------------------------------------------------------------------------------- |
| `on_tool_call`         | Runs before a tool executes. Can allow, stop, skip, rewrite arguments, or prompt the user. |
| `on_tool_result`       | Runs after a tool executes. Can inspect or rewrite tool output before the model sees it.   |
| `on_completion_call`   | Runs before prompt submission to the LLM provider.                                         |
| `on_invalid_tool_call` | Runs when an LLM requests a non-existent tool. Can retry with feedback or halt.            |
| `turn_start`           | Notifies turn start with the user's prompt.                                                |
| `turn_end`             | Notifies turn completion; `status` is `completed` or `content_filtered`.                   |

### How Hooks Work

1. When an event fires, `rho` runs the hook script as an isolated child process
   with a 5-second timeout.
2. `rho` pipes event details as single-line JSON to `stdin` and reads the
   decision JSON from `stdout`.
3. The hook process exits immediately; no persistent daemon runs in the
   background.
4. If a hook times out or exits non-zero without a valid action, execution fails
   closed for safety: a failing `tool_call` hook denies the call, and a failing
   `tool_result` hook replaces the tool output with a short "withheld" notice
   instead of passing it to the model.

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

- **[`rtk-rewrite.sh`](../examples/hooks/rtk-rewrite.sh)**: Intercepts `on_tool_call` and rewrites bash commands with `rtk rewrite` (needs `jq`) using [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk), compressing CLI output before it enters LLM context.
- **[`guard-bash.sh`](../examples/hooks/guard-bash.sh)**: Blocks destructive shell commands (`rm -rf`, `git reset --hard`) before execution.
- **[`python-audit.py`](../examples/hooks/python-audit.py)**: Audits tool call events to a local file.

### Using RTK with rho

[RTK](https://github.com/rtk-ai/rtk) (1.2 or newer) compresses the output of common commands (git, cargo, npm, test runners, `ls`, `grep`) before it reaches the model. rho has no built-in RTK step; the `on_tool_call` hook applies it:

```sh
mise use -g rtk@latest   # or: brew install rtk
mkdir -p ~/.agents/hooks
cp examples/hooks/rtk-rewrite.sh ~/.agents/hooks/on_tool_call
chmod +x ~/.agents/hooks/on_tool_call
```

The hook needs `jq`. It passes each `bash` command to `rtk rewrite` and acts on its exit code: `0` and `3` use the rewrite, `1` runs the command unchanged, and `2` with a message blocks the command with that message. Other tools are not touched. rho's permission rules still run afterwards and judge the original command, because an `rtk` prefix is ignored when matching. If `rtk` or `jq` is missing, commands run unchanged. Remove the hook file to turn it off.

If filtered output hides something you need, `rtk recall <hash>` prints it, and `rtk gain` shows the tokens saved.

