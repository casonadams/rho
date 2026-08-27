# Plugins and Extensions

`rho` features an extensible plugin and lifecycle hook system that integrates native Rust binaries, crates.io packages, and manifest-based plugins.

## Activation and discovery

`~/.config/rho/config.toml` is the only activation source for external executable plugins. A declaration may use an absolute path or a path relative to `config.toml`:

```toml
[plugins.container-bash]
path = "/opt/rho/rho-plugin-container-bash"
replaces = ["tool:bash"]

[plugins.review]
path = "plugins/rho-plugin-review"
replaces = []
```

A configured executable is trusted to run with the user's OS permissions. Plugin processes are not OS-sandboxed. Do not configure an executable unless you trust its code and installation path.

`rho plugin list` may also discover matching binaries in Cargo's bin directory, `PATH`, `~/.config/rho/plugins/`, and `.rho/plugins/`. Discovery is informational only: an undeclared binary is reported as unconfigured and is never started or allowed to contribute capabilities.

## Cargo installation and removal

A Cargo package must install a `rho-plugin-<name>` or `rho-<name>` executable. Installation is explicit and requires a local Cargo toolchain:

```bash
rho plugin install rho-plugin-review
rho plugin install rho-plugin-container-bash --replace tool:bash
rho plugin remove review
rho plugin list
rho plugin inspect tool:bash
```

`rho plugin install` runs Cargo, validates protocol-v1 discovery, and atomically writes the executable path, package metadata, and explicitly authorized replacements to `config.toml`. Validation failure leaves configuration unchanged and attempts to uninstall a newly installed package. `rho plugin remove` removes configuration before running Cargo uninstall. Removing a local-path declaration does not delete its executable.

Replacement requires both plugin metadata and the matching `--replace` authorization. Built-ins remain active when a plugin is missing, invalid, conflicting, or lacks replacement authorization.

## Protocol example

[`examples/capability_plugin.rs`](../examples/capability_plugin.rs) is a standalone protocol-v1 subprocess plugin with provider, tool, permission, command, lifecycle, and skill capabilities. It uses no network or credentials.

Build and configure it explicitly for local development:

```bash
cargo build --example capability_plugin
```

```toml
[plugins.fixture]
path = "../../target/debug/examples/capability_plugin"
replaces = []
```

The path is resolved relative to `config.toml`. Rho starts the executable only after it appears in `[plugins]`.

A global tool replacement uses a distinct capability identity and declares the built-in target in both plugin metadata and host configuration:

```toml
[plugins.container-shell]
path = "/opt/rho/rho-plugin-container-shell"
replaces = ["tool:bash"]
```

The replacement is advertised to the model as `bash`, while inspection reports its plugin and capability identities. Rho validates arguments, declared effects, protected paths, network targets, approval, repeated calls, and lifecycle events before dispatch. These checks govern model-requested operations; they do not sandbox the trusted executable itself.

## Legacy in-process extensions

The existing in-process extension API remains available during the capability migration. Legacy manifests are informational and do not authorize an external executable.

Internal capabilities and external plugin adapters implement the `Extension` lifecycle trait:

```rust
use async_trait::async_trait;
use rho::plugin::{Extension, ExtensionCommand, ExtensionContext, InputAction, ToolCallDecision, ToolCallEvent, ToolResultEvent, TurnEvent};

pub struct MyPlugin;

#[async_trait]
impl Extension for MyPlugin {
    fn name(&self) -> &str {
        "my_plugin"
    }

    /// Intercept, transform, or handle user input before the agent runs
    async fn on_input(&self, input: &str, _ctx: &ExtensionContext) -> rho::error::Result<InputAction> {
        if input == "ping" {
            return Ok(InputAction::Handled { output: "pong".to_string() });
        }
        Ok(InputAction::Continue)
    }

    /// Augment or modify system prompt and guidelines before each turn
    async fn before_turn(&self, event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        event.system_prompt.push_str("\n[Custom plugin instructions]");
        Ok(())
    }

    /// Inspect, allow, or block tool calls before execution
    async fn on_tool_call(&self, event: &ToolCallEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<ToolCallDecision> {
        if event.tool_name == "dangerous_tool" {
            return Ok(ToolCallDecision::Block {
                reason: "Operation blocked by plugin policy".to_string(),
                terminate: false,
            });
        }
        Ok(ToolCallDecision::Allow)
    }

    /// Inspect or post-process tool results after completion
    async fn on_tool_result(&self, event: &mut ToolResultEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        // Inspect or modify event.result
        Ok(())
    }

    /// Register custom slash commands callable in the REPL (e.g. `/mycmd`)
    fn register_commands(&self) -> Vec<ExtensionCommand> {
        vec![]
    }
}
```

---

## REPL & Command Dispatch

Custom commands registered by extensions appear automatically in the interactive `/help` menu and can be executed as slash commands:

```text
Commands
  /help                       Show this reference
  /model [model] [provider]   Inspect or switch the model
  /clear                      Start a new session; preserve history
  /login [provider]           Add API-key or subscription auth
  /logout [provider]          Remove stored provider auth
  /exit                       Exit rho

Extension commands
  /custom_echo                Echoes back the argument
```
