pub mod export;
pub mod help;
mod mcp;
pub mod model;
pub mod registry;
pub mod session;
pub mod skills;
#[cfg(test)]
mod tests;
pub mod types;
pub mod workflows;

use export::handle_export;
pub use help::print_help;
use help::{handle_help_command, handle_session};
use model::{handle_model, handle_thinking};
pub use registry::{
    BUILTIN_COMMANDS, BuiltinCommand, MidTurnPolicy, SHORTCUTS, SlashCommandSpec, lookup_builtin, mid_turn_policy,
};
use rho_harness_core::error::Result;
use session::{
    handle_compact_command, handle_login_cmd, handle_name_command, handle_resume_command, handle_rewind_command,
    handle_tree_command, handle_ui_modal_command, handle_worktree_command,
};
use skills::{handle_custom, handle_skill};
pub use types::{CommandResult, ForkTarget, SlashCommandContext};

pub fn is_slash_command(trimmed: &str) -> bool {
    if !trimmed.starts_with('/') {
        return false;
    }
    let Some(first_word) = trimmed[1..].split_whitespace().next() else {
        return false;
    };
    !first_word.contains('/') && !first_word.contains('\\') && !std::path::Path::new(trimmed).exists()
}

async fn dispatch_builtin(
    id: BuiltinCommand,
    parts: &[&str],
    raw_rest: &str,
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<CommandResult>> {
    let has_ui = ctx.renderer.has_interactive_ui();
    match id {
        BuiltinCommand::Help => Ok(Some(handle_help_command(parts, ctx))),
        BuiltinCommand::Settings => Ok(Some(handle_ui_modal_command(
            has_ui,
            CommandResult::OpenSettingsSelector,
            "  Settings: thinking effort, thinking blocks, tool outputs\n",
            ctx.renderer,
        ))),
        BuiltinCommand::Models => Ok(Some(handle_ui_modal_command(
            has_ui,
            CommandResult::OpenModelsSelector,
            "  Role models require interactive TUI mode (or edit config.toml)\n",
            ctx.renderer,
        ))),
        BuiltinCommand::Model => handle_model(ctx, parts),
        BuiltinCommand::Thinking => handle_thinking(ctx, parts),
        BuiltinCommand::Tools => Ok(Some(handle_ui_modal_command(
            has_ui,
            CommandResult::OpenToolsSelector,
            "  Tools & permissions selector requires interactive TUI mode\n",
            ctx.renderer,
        ))),
        BuiltinCommand::Agents => Ok(Some(handle_ui_modal_command(
            has_ui,
            CommandResult::OpenAgentsSelector,
            "  Agent monitor requires interactive TUI mode\n",
            ctx.renderer,
        ))),
        BuiltinCommand::Btw => Ok(Some(CommandResult::Btw {
            question: (!raw_rest.is_empty()).then(|| raw_rest.to_string()),
        })),
        BuiltinCommand::Resume => Ok(Some(handle_resume_command(parts, has_ui, ctx.renderer))),
        BuiltinCommand::Skill => handle_skill(ctx, parts).await,
        BuiltinCommand::Mcp => mcp::handle_mcp(ctx, parts).await,
        BuiltinCommand::Session => {
            handle_session(ctx);
            Ok(Some(CommandResult::Continue))
        }
        BuiltinCommand::Recap => Ok(Some(CommandResult::Recap)),
        BuiltinCommand::Compact => Ok(Some(handle_compact_command(parts))),
        BuiltinCommand::Tree => Ok(Some(handle_tree_command(has_ui))),
        BuiltinCommand::Fork => Ok(Some(parse_fork_command(parts))),
        BuiltinCommand::Clone => Ok(Some(CommandResult::CloneSession)),
        BuiltinCommand::Name => Ok(Some(handle_name_command(parts, ctx.renderer))),
        BuiltinCommand::Rewind => Ok(Some(handle_rewind_command(parts, ctx.renderer))),
        BuiltinCommand::Clear => {
            ctx.renderer.print_status("Context cleared");
            Ok(Some(CommandResult::ClearContext))
        }
        BuiltinCommand::Login => Ok(Some(handle_login_cmd(parts))),
        BuiltinCommand::Logout => Ok(Some(CommandResult::Logout {
            provider: parts.get(1).map(|v| (*v).to_string()),
        })),
        BuiltinCommand::Reload => Ok(Some(CommandResult::Reload)),
        BuiltinCommand::Export => handle_export(ctx, parts).await,
        BuiltinCommand::Workflow => Ok(Some(workflows::handle_workflow(ctx, parts).await)),
        BuiltinCommand::Worktree => Ok(Some(handle_worktree_command(parts, ctx.renderer))),
        BuiltinCommand::Exit => {
            ctx.renderer.print_notice("  Bye!\n");
            Ok(Some(CommandResult::Exit))
        }
    }
}

fn parse_fork_command(parts: &[&str]) -> CommandResult {
    match parts.get(1).copied() {
        Some(first) => match ForkTarget::parse(first) {
            Some(target) => CommandResult::ForkSession {
                target: Some(target),
                turn_or_node_id: parts.get(2).map(|s| (*s).to_string()),
            },
            None => CommandResult::ForkSession {
                target: None,
                turn_or_node_id: Some(first.to_string()),
            },
        },
        None => CommandResult::ForkSession {
            target: None,
            turn_or_node_id: None,
        },
    }
}

pub struct SlashCommandHandler;

impl SlashCommandHandler {
    pub async fn handle(input: &str, ctx: &mut SlashCommandContext<'_>) -> Result<Option<CommandResult>> {
        let trimmed = input.trim();
        if !is_slash_command(trimmed) {
            return Ok(None);
        }

        let words = trimmed[1..].split_whitespace().next();
        let tokens: Vec<String> = if words.is_some_and(|w| w.eq_ignore_ascii_case("workflow")) {
            workflows::split_args(&trimmed[1..])
        } else {
            trimmed[1..].split_whitespace().map(str::to_string).collect()
        };
        let parts: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let Some(&first) = parts.first() else {
            return Ok(None);
        };
        let cmd = first.to_lowercase();

        if let Some(spec) = lookup_builtin(&cmd) {
            let rest = trimmed[1 + first.len()..].trim();
            return dispatch_builtin(spec.id, &parts, rest, ctx).await;
        }
        handle_custom(ctx, &cmd, &parts).await
    }
}
