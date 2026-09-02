# Interactive Python Guard Plugin for `rho`

A full interactive security guard plugin written in Python demonstrating how any programming language can intercept all tool calls and request user approval via `rho`'s Host UI services.

## Capabilities

- Intercepts **every tool call** (`bash`, `read`, `write`, `edit`, `search`, `fetch`).
- Displays a native interactive confirmation modal via `host/ui/confirm` showing the tool name and argument summary.
- If approved, proceeds with execution (`{"action": "continue"}`).
- If denied, returns a clear denial notice (`{"action": "skip", "reason": "..."}`) telling the LLM not to retry.
- Automatically repairs hallucinated tool names (e.g. `sh` $\to$ `bash`).

## Configuration in `config.toml`

Add to `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[plugins.python_guard]
enabled = true
command = "python3"
args = ["/path/to/rho/examples/plugins/python-guard/guard.py"]
```
