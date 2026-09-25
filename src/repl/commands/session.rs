use super::help::{handle_session, print_help};
use super::types::{CommandResult, SlashCommandContext};
use crate::ui::TerminalRenderer;

pub(crate) fn handle_tree_slash_commands(name: &str, parts: &[&str], has_ui: bool) -> Option<CommandResult> {
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

pub(crate) fn handle_rewind_command(parts: &[&str], renderer: &TerminalRenderer) -> CommandResult {
    match parts.get(1).and_then(|p| p.parse::<usize>().ok()) {
        Some(turn) => CommandResult::Rewind { turn },
        None => {
            renderer.print_notice("  Usage: /rewind <turn_number> (e.g. /rewind 2)\n");
            CommandResult::Continue
        }
    }
}

pub(crate) fn handle_resume_command(parts: &[&str], has_ui: bool, renderer: &TerminalRenderer) -> CommandResult {
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

pub(crate) fn handle_session_slash_commands(
    name: &str,
    parts: &[&str],
    renderer: &TerminalRenderer,
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

pub(crate) fn handle_settings_command(has_ui: bool, renderer: &TerminalRenderer) -> CommandResult {
    if has_ui {
        CommandResult::OpenSettingsSelector
    } else {
        renderer.print_notice("  Settings: thinking effort, thinking blocks, tool outputs\n");
        CommandResult::Continue
    }
}

pub(crate) fn handle_simple_slash_commands(name: &str, ctx: &mut SlashCommandContext<'_>) -> Option<CommandResult> {
    match name {
        "help" => {
            if ctx.renderer.has_interactive_ui() {
                Some(CommandResult::OpenHelpSelector)
            } else {
                print_help(ctx.config, ctx.renderer);
                Some(CommandResult::Continue)
            }
        }
        "clear" | "reset" | "new" => {
            ctx.renderer.print_status("Context cleared");
            Some(CommandResult::ClearContext)
        }
        "session" | "tokens" => {
            handle_session(ctx);
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

pub(crate) fn handle_login_cmd(parts: &[&str]) -> CommandResult {
    match parts.get(1) {
        Some(p) => CommandResult::Login {
            provider: Some((*p).to_string()),
        },
        None => CommandResult::OpenLoginSelector,
    }
}
