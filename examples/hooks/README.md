# rho Lifecycle Hooks

rho lifecycle hooks are simple, one-shot executables placed in
`<workspace>/.agents/hooks/` (or user fallback `~/.agents/hooks/`). When agent
events occur, rho pipes event details as single-line JSON to the script's
`stdin` and reads the decision from `stdout`.

Hooks run without long-lived background daemons, without network sockets, and
without custom SDKs. You can write hooks in Bash, Python, Node, Go, Rust, or any
executable format.

## Hook Events

Place an executable matching the event name in `.agents/hooks/`:

| Event Script                                 | Description                                                                                          |
| :------------------------------------------- | :--------------------------------------------------------------------------------------------------- |
| `on_tool_call` / `tool_call`                 | Intercept tool calls before execution. Can allow, stop, skip, rewrite arguments, or prompt the user. |
| `on_tool_result` / `tool_result`             | Inspect or rewrite tool results before the model receives them.                                      |
| `on_completion_call` / `completion_call`     | Intercept LLM prompt submissions or halt execution.                                                  |
| `on_invalid_tool_call` / `invalid_tool_call` | Handle invalid tool calls (retry with feedback or halt).                                             |
| `turn_start`                                 | Observe turn start with initial user prompt and session metadata.                                    |
| `turn_end`                                   | Observe turn end with tool counts and completion status.                                             |

## Hook Decisions (`stdout`)

A hook script exits with status `0` and can output a JSON decision on `stdout`:

```json
// 1. Continue execution normally (or exit 0 with empty stdout)
{"action": "continue"}

// 2. Stop the agent run immediately
{"action": "stop", "reason": "Destructive commands forbidden"}

// 3. Skip this tool call
{"action": "skip", "reason": "Tool skipped by policy"}

// 4. Rewrite tool arguments before execution
{"action": "rewrite_args", "args": {"command": "git status"}}

// 5. Ask the user via rho's native TUI modal
{"action": "ask", "message": "Sensitive file access detected. Allow?"}
```

## Example Hooks

The `examples/hooks/` directory contains ready-to-use hooks:

- **[`rtk-rewrite.py`](rtk-rewrite.py)**: Automatically routes bash commands through [RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) to filter and compress verbose CLI output (saving 50–90% on context tokens) before passing results to the LLM. Visit [github.com/rtk-ai/rtk](https://github.com/rtk-ai/rtk) for RTK install instructions.
- **[`guard-bash.sh`](guard-bash.sh)**: Blocks potentially destructive shell commands (such as `rm -rf`) before execution.
- **[`python-audit.py`](python-audit.py)**: Audits tool calls and parameters to a local log file.

### Installing a Hook

Copy any hook to your project's `.agents/hooks/` directory (or user fallback `~/.agents/hooks/`) matching the target lifecycle event name, and ensure it is executable:

```sh
# Project-level hook
mkdir -p .agents/hooks
cp examples/hooks/rtk-rewrite.py .agents/hooks/on_tool_call
chmod +x .agents/hooks/on_tool_call

# Or global user hook
mkdir -p ~/.agents/hooks
cp examples/hooks/rtk-rewrite.py ~/.agents/hooks/on_tool_call
chmod +x ~/.agents/hooks/on_tool_call
```

