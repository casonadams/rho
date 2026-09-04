use crate::repl::interactive::{CompletionSet, InteractiveHistory};
use crate::ui::interactive::{TerminalBackend, TerminalController};

pub fn navigate_history_previous<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_up(width) {
        return true;
    }
    let Some(value) = history.previous(controller.state().editor().text()) else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

pub fn navigate_history_next<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    history: &mut InteractiveHistory,
) -> bool {
    let width = controller.terminal_width();
    if controller.state_mut().editor_mut().move_down(width) {
        return true;
    }
    let Some(value) = history.next_entry() else {
        return false;
    };
    controller.state_mut().editor_mut().set_text(value);
    true
}

pub fn apply_completion<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) -> bool {
    apply_completion_generic(controller, completions)
}

pub fn apply_completion_generic<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) -> bool {
    let editor = controller.state().editor();
    let text = editor.text();
    let byte_index = editor.cursor();
    let candidates = completions.complete(text, byte_index);
    let Some(first) = candidates.first() else {
        return false;
    };
    let mut updated = String::new();
    updated.push_str(&text[..first.replacement.start]);
    updated.push_str(&first.value);
    if !first.value.ends_with(' ') {
        updated.push(' ');
    }
    updated.push_str(&text[first.replacement.end..]);
    controller.state_mut().editor_mut().set_text(&updated);
    true
}

pub fn restore_queued_messages<B: TerminalBackend>(controller: &mut TerminalController<B>) {
    let mut restored = Vec::new();
    while let Some(message) = controller.state_mut().pop_queued() {
        restored.push(message.text);
    }
    if !restored.is_empty() {
        controller.state_mut().editor_mut().set_text(restored.join("\n\n"));
    }
}
