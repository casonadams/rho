use crossterm::event::KeyEvent;

use super::super::ModalKeyResult;
use crate::error::Result;
use crate::repl::commands::{BUILTIN_COMMANDS, BuiltinCommand, SHORTCUTS};
use crate::ui::interactive::{ModalKind, ModalOption, ModalState, TerminalBackend, TerminalController};

fn help_modal_options() -> Vec<ModalOption> {
    let mut options = Vec::new();
    for spec in BUILTIN_COMMANDS {
        if spec.id == BuiltinCommand::Help {
            continue;
        }
        let desc = if spec.aliases.is_empty() {
            spec.summary.to_string()
        } else {
            let aliases = spec
                .aliases
                .iter()
                .map(|a| format!("/{a}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} (alias: {aliases})", spec.summary)
        };
        let label = format!("/{}", spec.name);
        options.push(ModalOption::new(format!("{label:<15}"), Some(desc)));
        for sub in spec.subcommands {
            options.push(ModalOption::new(format!("{:<15}", sub.usage), Some(sub.summary)));
        }
    }
    for shortcut in SHORTCUTS {
        options.push(ModalOption::new(
            format!("{:<15}", shortcut.key),
            Some(shortcut.summary),
        ));
    }
    options
}

pub fn open_help_selector<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let modal = ModalState::new("Help", "", help_modal_options())
        .with_search(true)
        .with_kind(ModalKind::Help);
    controller.state_mut().push_modal(modal);
}

pub fn handle_help_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    key: KeyEvent,
) -> Result<ModalKeyResult> {
    crate::repl::live::modal::dispatch_simple_selector(controller, key, |opt| {
        let raw = opt.label.trim();
        raw.starts_with('/').then(|| ModalKeyResult::HelpCommandSelected {
            command: raw.to_string(),
        })
    })
}
