# Python Security Guard Plugin for `rho`

An example security guard plugin written in Python demonstrating how any programming language can communicate with `rho` via standard JSON-RPC 2.0.

## Capabilities

- Blocks destructive `rm -rf /` commands with an explanatory policy reason.
- Asynchronously triggers native terminal confirmation modals via `host/ui/confirm` when privileged commands (`sudo`, `reboot`) are called.
- Automatically repairs hallucinated tool names (e.g. `sh` $\to$ `bash`).

## Configuration in `config.toml`

Add to `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[plugins.python_guard]
enabled = true
command = "python3"
args = ["/path/to/rho/examples/plugins/python-guard/guard.py"]
```
