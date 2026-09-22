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

## Example: Bash Command Guard (`.agents/hooks/on_tool_call`)

```sh
#!/bin/sh
# Read event JSON from stdin
read -r EVENT

# Block rm -rf commands
if echo "$EVENT" | grep -q '"rm -rf'; then
  echo '{"action": "stop", "reason": "rm -rf is blocked by policy"}'
  exit 0
fi

# Allow all other calls
exit 0
```

Remember to make the script executable: `chmod +x .agents/hooks/on_tool_call`.

## Example: RTK Token Optimization (`.agents/hooks/on_tool_call`)

Automatically routes bash commands through
[RTK (Rust Token Killer)](https://github.com/rtk-ai/rtk) to filter and compress
verbose CLI output (saving 50-90% on command output tokens) before passing
results to the LLM.

For RTK installation instructions and options, visit
[github.com/rtk-ai/rtk](https://github.com/rtk-ai/rtk).

```python
#!/usr/bin/env python3
# Place in .agents/hooks/on_tool_call and chmod +x
import json
import subprocess
import sys


def rewrite_command(cmd):
    if not cmd or cmd.startswith("rtk "):
        return None
    res = subprocess.run(["rtk", "rewrite", cmd], capture_output=True, text=True)
    rewritten = res.stdout.strip()
    if res.returncode in (0, 3) and rewritten and rewritten != cmd:
        return rewritten
    return None


def main():
    try:
        event = json.load(sys.stdin)
        if event.get("tool_name") != "bash":
            return

        args = event.get("args") or {}
        cmd = args.get("command", "")
        rewritten = rewrite_command(cmd)
        if rewritten:
            args["command"] = rewritten
            print(json.dumps({"action": "rewrite_args", "args": args}))
    except Exception:
        pass


if __name__ == "__main__":
    main()
```
