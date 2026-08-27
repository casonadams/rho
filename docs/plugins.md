# Plugins and Extensions

`rho` features an extensible plugin and lifecycle hook system that integrates native Rust binaries, crates.io packages, and manifest-based plugins.

## Discovery Locations

When `rho` starts or when you run `rho plugin list`, `PluginLoader` discovers plugins across the following locations:

1. **`~/.cargo/bin/`** — Standard Cargo binary install directory.
2. **`$PATH`** — All directories in the system environment path.
3. **`~/.config/rho/plugins/`** — Global plugin directory and manifests.
4. **`.rho/plugins/`** — Project-local plugins and manifests.

---

## Cargo & Crates.io Plugins

You can author standalone plugin crates in Rust and publish them to [crates.io](https://crates.io).

### 1. Binary Naming Conventions
Any binary matching either naming convention is automatically recognized:
- `rho-plugin-<name>` (e.g. `rho-plugin-review`, `rho-plugin-git`)
- `rho-<name>` (e.g. `rho-review`, `rho-git`)

### 2. Installing Plugins
Install crates from crates.io directly with Cargo or via `rho`:

```bash
# Using cargo
cargo install rho-plugin-review

# Using rho CLI
rho plugin install rho-plugin-review

# Inspect discovered plugins
rho plugin list
```

---

## Manifest-Based Plugins

Plugins can also be placed in dedicated folders under `~/.config/rho/plugins/<plugin-name>/` or `.rho/plugins/<plugin-name>/` with a `plugin.toml` manifest:

```toml
name = "git-enhancements"
version = "0.1.0"
description = "Git status and checkpointing tools for rho"
author = "Your Name"
binary = "rho-plugin-git"
```

---

## Lifecycle Hooks & Extension Trait

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
