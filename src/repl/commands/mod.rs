pub mod args;
pub mod export;
pub mod help;
pub mod model;
pub mod session;
pub mod skills;
#[cfg(test)]
mod tests;
pub mod types;

use export::handle_export;
pub use help::print_help;
use model::{handle_model, handle_thinking};
use rho_harness_core::error::Result;
use session::{
    handle_login_cmd, handle_session_slash_commands, handle_simple_slash_commands, handle_tree_slash_commands,
};
use skills::{handle_custom, handle_skill};
pub use types::{CommandResult, SLASH_COMMANDS, SlashCommandContext};

pub fn is_slash_command(trimmed: &str) -> bool {
    if !trimmed.starts_with('/') {
        return false;
    }
    let Some(first_word) = trimmed[1..].split_whitespace().next() else {
        return false;
    };
    !first_word.contains('/') && !first_word.contains('\\') && !std::path::Path::new(trimmed).exists()
}

async fn handle_async_slash_commands(
    name: &str,
    parts: &[&str],
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<CommandResult>> {
    match name {
        "thinking" | "think" => handle_thinking(ctx, parts),
        "model" => handle_model(ctx, parts),
        "skill" | "skills" => handle_skill(ctx, parts).await,
        "mcp" => {
            if parts.get(1) == Some(&"login") {
                let target = parts.get(2).map(|s| format!("mcp:{s}"));
                Ok(Some(CommandResult::Login { provider: target }))
            } else if ctx.renderer.has_interactive_ui() {
                Ok(Some(CommandResult::OpenMcpSelector))
            } else {
                ctx.renderer.print_notice("  Configure MCP in ~/.config/rho/mcp.json\n");
                Ok(Some(CommandResult::Continue))
            }
        }
        "login" => Ok(Some(handle_login_cmd(parts))),
        "logout" => Ok(Some(CommandResult::Logout {
            provider: parts.get(1).map(|v| (*v).to_string()),
        })),
        "export" => handle_export(ctx, parts).await,
        custom => handle_custom(ctx, custom, parts).await,
    }
}

pub struct SlashCommandHandler;

impl SlashCommandHandler {
    pub async fn handle(input: &str, ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
        let trimmed = input.trim();
        if !is_slash_command(trimmed) {
            return Ok(None);
        }

        let parts: Vec<&str> = trimmed[1..].split_whitespace().collect();
        let Some(&first) = parts.first() else {
            return Ok(None);
        };
        let cmd = first.to_lowercase();

        if let Some(res) = handle_simple_slash_commands(&cmd, ctx) {
            return Ok(Some(res));
        }
        if let Some(res) = handle_tree_slash_commands(&cmd, &parts, ctx.renderer.has_interactive_ui()) {
            return Ok(Some(res));
        }
        if let Some(res) = handle_session_slash_commands(&cmd, &parts, ctx.renderer) {
            return Ok(Some(res));
        }
        handle_async_slash_commands(&cmd, &parts, ctx).await
    }
}
