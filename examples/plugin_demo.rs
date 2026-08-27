use async_trait::async_trait;
use rho::plugin::{
    CommandHandler, CommandRequest, Extension, ExtensionCommand, ExtensionContext, ExtensionRegistry, InputAction,
    ToolCallDecision, ToolCallEvent, ToolResultEvent, TurnEvent,
};
use std::sync::Arc;

/// A demo plugin demonstrating all lifecycle hooks.
pub struct GuardPlugin;

#[async_trait]
impl Extension for GuardPlugin {
    fn name(&self) -> &str {
        "guard_plugin"
    }

    async fn on_session_start(&self, ctx: &ExtensionContext) -> rho::error::Result<()> {
        println!("[guard_plugin] Session started for model: {}", ctx.model);
        Ok(())
    }

    async fn on_input(&self, input: &str, _ctx: &ExtensionContext) -> rho::error::Result<InputAction> {
        if input == "ping" {
            return Ok(InputAction::Handled {
                output: "pong from guard_plugin".to_string(),
            });
        }
        if let Some(help) = input.strip_prefix("?help ") {
            return Ok(InputAction::Transform(format!("Explain clearly: {help}")));
        }
        Ok(InputAction::Continue)
    }

    async fn before_turn(&self, event: &mut TurnEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        event
            .system_prompt
            .push_str("\n[Rule from guard_plugin: Never expose private API keys]");
        Ok(())
    }

    async fn on_tool_call(
        &self,
        event: &ToolCallEvent<'_>,
        _ctx: &ExtensionContext,
    ) -> rho::error::Result<ToolCallDecision> {
        if event.tool_name == "bash"
            && let Some(cmd) = event.args.get("command").and_then(|c| c.as_str())
            && cmd.contains("rm -rf /")
        {
            return Ok(ToolCallDecision::Block {
                reason: "Destructive root filesystem operation blocked".to_string(),
                terminate: false,
            });
        }
        Ok(ToolCallDecision::Allow)
    }

    async fn on_tool_result(&self, event: &mut ToolResultEvent<'_>, _ctx: &ExtensionContext) -> rho::error::Result<()> {
        if event.tool_name == "read" {
            // Can inspect or post-process tool result
        }
        Ok(())
    }

    fn register_commands(&self) -> Vec<ExtensionCommand> {
        vec![ExtensionCommand {
            name: "guard_status".to_string(),
            description: "Check guard plugin status".to_string(),
            handler: Arc::new(GuardStatusCommandHandler),
        }]
    }
}

struct GuardStatusCommandHandler;

#[async_trait]
impl CommandHandler for GuardStatusCommandHandler {
    async fn execute(&self, _args: &str, ctx: &ExtensionContext) -> rho::error::Result<String> {
        Ok(format!(
            "Guard plugin active in {} (trusted: {})",
            ctx.cwd.display(),
            ctx.is_trusted
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize registry and register plugin
    let mut registry = ExtensionRegistry::new();
    registry.register(GuardPlugin);

    let ctx = ExtensionContext::new(".", "demo_session").with_model_info("gpt-5.6-luna", "chatgpt");

    // 2. Test session startup
    registry.dispatch_session_start(&ctx).await?;

    // 3. Test input interception
    let action = registry.dispatch_input("ping", &ctx).await?;
    println!("Input 'ping' result: {:?}", action);

    // 4. Test tool call guard
    let blocked_args = serde_json::json!({ "command": "rm -rf /" });
    let blocked = registry
        .dispatch_tool_call(
            &ToolCallEvent {
                tool_name: "bash",
                args: &blocked_args,
            },
            &ctx,
        )
        .await?;
    println!("Tool call decision on rm -rf /: {:?}", blocked);

    // 5. Test custom slash command
    let cmd_output = registry
        .dispatch_command(
            &CommandRequest {
                name: "guard_status",
                args: "",
            },
            &ctx,
        )
        .await
        .unwrap()?;
    println!("Slash command /guard_status output: {}", cmd_output);

    Ok(())
}
