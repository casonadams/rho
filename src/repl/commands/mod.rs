pub mod args;
mod custom;
mod export;
pub mod help;
mod model;
mod plugin;
mod session;
mod skill;
mod theme;
mod thinking;
mod types;

#[cfg(test)]
mod tests;

pub use types::{CommandResult, SLASH_COMMANDS, SlashCommandContext};

use help::print_help;
use rho_harness_core::error::Result;

pub fn is_slash_command(trimmed: &str) -> bool {
    if !trimmed.starts_with('/') {
        return false;
    }
    let Some(first_word) = trimmed[1..].split_whitespace().next() else {
        return false;
    };
    !first_word.contains('/') && !first_word.contains('\\') && !std::path::Path::new(trimmed).exists()
}

fn handle_tree_slash_commands(name: &str, parts: &[&str], has_ui: bool) -> Option<CommandResult> {
    match name {
        "tree" => Some(if has_ui {
            CommandResult::OpenTreeSelector
        } else {
            CommandResult::Tree
        }),
        "fork" => Some(CommandResult::ForkSession {
            turn_or_node_id: parts.get(1).map(|s| s.to_string()),
        }),
        "clone" => Some(CommandResult::CloneSession),
        "compact" => {
            let instructions = parts.get(1..).filter(|s| !s.is_empty()).map(|s| s.join(" "));
            Some(CommandResult::Compact { instructions })
        }
        _ => None,
    }
}

fn handle_rewind_command(parts: &[&str], renderer: &crate::ui::TerminalRenderer) -> CommandResult {
    match parts.get(1).and_then(|p| p.parse::<usize>().ok()) {
        Some(turn) => CommandResult::Rewind { turn },
        None => {
            renderer.print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
            CommandResult::Continue
        }
    }
}

fn handle_resume_command(parts: &[&str], has_ui: bool, renderer: &crate::ui::TerminalRenderer) -> CommandResult {
    match parts.get(1) {
        Some(id) => CommandResult::ResumeSession {
            session_id: (*id).to_string(),
        },
        None if has_ui => CommandResult::OpenSessionSelector,
        None => {
            renderer.print_notice("  Usage: /resume <session_id>\n");
            CommandResult::Continue
        }
    }
}

fn handle_session_slash_commands(
    name: &str,
    parts: &[&str],
    renderer: &crate::ui::TerminalRenderer,
) -> Option<CommandResult> {
    match name {
        "rewind" => Some(handle_rewind_command(parts, renderer)),
        "name" => match parts.get(1..) {
            Some(slice) if !slice.is_empty() => Some(CommandResult::NameSession { name: slice.join(" ") }),
            _ => {
                renderer.print_notice("  Usage: /name <session_name>\n");
                Some(CommandResult::Continue)
            }
        },
        "resume" => Some(handle_resume_command(parts, renderer.has_interactive_ui(), renderer)),
        _ => None,
    }
}

fn handle_settings_command(has_ui: bool, renderer: &crate::ui::TerminalRenderer) -> CommandResult {
    if has_ui {
        CommandResult::OpenSettingsSelector
    } else {
        renderer.print_notice("  Settings: thinking blocks, tool outputs\n");
        CommandResult::Continue
    }
}

fn handle_simple_slash_commands(name: &str, ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    match name {
        "help" => {
            print_help(ctx.config, ctx.renderer);
            Some(CommandResult::Continue)
        }
        "clear" | "reset" | "new" => {
            ctx.renderer.print_status("Context cleared");
            Some(CommandResult::ClearContext)
        }
        "session" => {
            session::handle_session(ctx);
            Some(CommandResult::Continue)
        }
        "settings" => Some(handle_settings_command(ctx.renderer.has_interactive_ui(), ctx.renderer)),
        "reload" => Some(CommandResult::Reload),
        "exit" | "quit" => {
            ctx.renderer.print_notice("  Bye!\n");
            Some(CommandResult::Exit)
        }
        _ => None,
    }
}

fn handle_login_cmd(parts: &[&str]) -> CommandResult {
    match parts.get(1) {
        Some(p) => CommandResult::Login {
            provider: Some((*p).to_string()),
        },
        None => CommandResult::OpenLoginSelector,
    }
}

async fn handle_async_slash_commands(
    name: &str,
    parts: &[&str],
    ctx: &mut SlashCommandContext<'_>,
) -> Result<Option<CommandResult>> {
    match name {
        "thinking" | "think" => thinking::handle_thinking(ctx, parts),
        "model" => model::handle_model(ctx, parts),
        "theme" => theme::handle_theme(ctx, parts),
        "skill" | "skills" => skill::handle_skill(ctx, parts).await,
        "plugin" | "plugins" => Ok(plugin::handle_plugins(ctx)),
        "login" => Ok(Some(handle_login_cmd(parts))),
        "logout" => Ok(Some(CommandResult::Logout {
            provider: parts.get(1).map(|v| (*v).to_string()),
        })),
        "export" => export::handle_export(ctx, parts).await,
        custom => custom::handle_custom(ctx, custom, parts).await,
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
