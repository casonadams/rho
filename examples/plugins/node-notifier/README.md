# Node.js Notifier Plugin for `rho`

An example plugin written in Node.js demonstrating how external JavaScript/TypeScript scripts hook into `rho`'s lifecycle events and emit real-time notifications via Host Services.

## Capabilities

- Subscribes to `hook/tool_call` and `hook/tool_result`.
- Emits real-time notification toasts into `rho`'s transcript via `host/ui/notify`.
- Allows tools to continue normally (`{"action": "continue"}`).

## Configuration in `config.toml`

Add to `~/.config/rho/config.toml` or `.rho/config.toml`:

```toml
[plugins.node_notifier]
enabled = true
command = "node"
args = ["/path/to/rho/examples/plugins/node-notifier/notifier.js"]
```

## Example Output

When `rho` runs any command (e.g. `git status`), the plugin will output:

```text
The user wants me to run git status.
[Node.js Notifier] Starting tool 'bash'...
[Node.js Notifier] Finished 'bash' (400 chars output)

bash `git status`
...
Took 47ms
```
