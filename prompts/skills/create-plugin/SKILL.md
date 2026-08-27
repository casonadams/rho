---
name: create-plugin
description: Create, test, and package a plugin or extension for rho. Use when asked to write a plugin, extension, guard, or custom command for rho.
argument-hint: "<plugin-idea-or-specification>"
---

# Creating a Plugin for `rho`

When authoring a plugin for `rho`:

## 1. Quick Template

A `rho` plugin implements the `rho::plugin::Extension` trait:

```rust
use async_trait::async_trait;
use rho::plugin::{
    CommandHandler, CommandRequest, Extension, ExtensionCommand, ExtensionContext, ExtensionRegistry,
    InputAction, ToolCallDecision, ToolCallEvent, ToolResultEvent, TurnEvent,
};
use std::sync::Arc;

pub struct MyPlugin;

#[async_trait]
impl Extension for MyPlugin {
    fn name(&self) -> &str {
        "my_plugin"
    }

    /// Intercept, transform, or short-circuit user prompts
    async fn on_input(&self, input: &str, _ctx: &ExtensionContext) -> rho::error::Result<InputAction> {
        if input == "ping" {
            return Ok(InputAction::Handled { output: "pong".to_string() });
        }
        Ok(InputAction::Continue)
    }

    /// Augment system prompt before model turns
    async fn before_turn(&self, event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        event.system_prompt.push_str("\n[Custom instruction from my_plugin]");
        Ok(())
    }

    /// Inspect or block tool execution
    async fn on_tool_call(
        &self,
        event: &ToolCallEvent<'_>,
        _ctx: &ExtensionContext,
    ) -> rho::error::Result<ToolCallDecision> {
        if event.tool_name == "bash"
            && let Some(cmd) = event.args.get("command").and_then(|c| c.as_str())
            && cmd.contains("dangerous_command")
        {
            return Ok(ToolCallDecision::Block {
                reason: "Command blocked by security policy".to_string(),
                terminate: false,
            });
        }
        Ok(ToolCallDecision::Allow)
    }

    /// Post-process tool results
    async fn on_tool_result(&self, _event: &mut ToolResultEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        Ok(())
    }

    /// Register slash commands in the REPL (e.g. `/mycommand`)
    fn register_commands(&self) -> Vec<ExtensionCommand> {
        vec![ExtensionCommand {
            name: "mycommand".to_string(),
            description: "A custom command".to_string(),
            handler: Arc::new(MyCommandHandler),
        }]
    }
}

struct MyCommandHandler;

#[async_trait]
impl CommandHandler for MyCommandHandler {
    async fn execute(&self, args: &str, ctx: &ExtensionContext) -> rho::error::Result<String> {
        Ok(format!("Executed in {} with args: {args}", ctx.cwd.display()))
    }
}
```

## 2. Testing in Isolation

Plugin authors can test all hooks deterministically in standard Rust unit tests without launching the full agent or needing network calls:

```rust
#[tokio::test]
async fn test_my_plugin_isolated() {
    let mut registry = ExtensionRegistry::new();
    registry.register(MyPlugin);
    let ctx = ExtensionContext::new(".", "test_session");

    // Test input handling
    let action = registry.dispatch_input("ping", &ctx).await.unwrap();
    assert_eq!(action, InputAction::Handled { output: "pong".to_string() });

    // Test command
    let res = registry.dispatch_command(&CommandRequest { name: "mycommand", args: "test" }, &ctx).await.unwrap().unwrap();
    assert!(res.contains("test"));
}
```

## 3. Distribution & Publishing to crates.io

1. Set up a cargo binary crate:
   - Package name: `rho-plugin-<name>` or `rho-<name>`
   - `[[bin]] name = "rho-plugin-<name>"`
2. Publish with `cargo publish`.
3. Users install with `cargo install rho-plugin-<name>` (or `rho plugin install rho-plugin-<name>`).
