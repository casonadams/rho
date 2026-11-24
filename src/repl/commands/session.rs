use super::types::CommandResult;
use crate::ui::TerminalRenderer;

pub(crate) fn handle_tree_command(has_ui: bool) -> CommandResult {
    if has_ui {
        CommandResult::OpenTreeSelector
    } else {
        CommandResult::Tree
    }
}

pub(crate) fn handle_compact_command(parts: &[&str]) -> CommandResult {
    let instructions = parts.get(1..).filter(|s| !s.is_empty()).map(|s| s.join(" "));
    CommandResult::Compact { instructions }
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

pub(crate) fn handle_name_command(parts: &[&str], renderer: &TerminalRenderer) -> CommandResult {
    match parts.get(1..) {
        Some(slice) if !slice.is_empty() => CommandResult::NameSession { name: slice.join(" ") },
        _ => {
            renderer.print_notice("  Usage: /name <session_name>\n");
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

pub(crate) fn handle_ui_modal_command(
    has_ui: bool,
    modal: CommandResult,
    fallback: &str,
    renderer: &TerminalRenderer,
) -> CommandResult {
    if has_ui {
        modal
    } else {
        renderer.print_notice(fallback);
        CommandResult::Continue
    }
}

pub(crate) fn handle_worktree_command(parts: &[&str], renderer: &TerminalRenderer) -> CommandResult {
    if parts.len() > 2 {
        renderer.print_notice("Usage: /wt [<branch>]\n");
        return CommandResult::Continue;
    }
    CommandResult::EnterWorktree {
        branch: parts.get(1).map(|v| (*v).to_string()),
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
