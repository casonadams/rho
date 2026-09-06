use super::navigation::apply_completion_generic;
use crate::repl::interactive::CompletionSet;
use crate::ui::interactive::{TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum AutocompleteKeyResult {
    Handled,
    NotHandled,
}

fn apply_selected_completion<B: TerminalBackend>(controller: &mut TerminalController<B>, val: &str) {
    let state = controller.state_mut();
    let editor = state.editor_mut();
    let text = editor.text();
    let cursor = editor.cursor();
    if val.starts_with('/') {
        let mut new_text = val.to_string();
        if !new_text.ends_with(' ') {
            new_text.push(' ');
        }
        new_text.push_str(&text[cursor..]);
        editor.set_text(&new_text);
    } else {
        let end = text[cursor..].find(' ').map_or(text.len(), |i| cursor + i);
        let new_text = format!("{val} {}", &text[end..]);
        editor.set_text(&new_text);
    }
}

fn handle_accept_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) -> AutocompleteKeyResult {
    let selected_val = controller
        .state_mut()
        .autocomplete
        .selected_item()
        .map(|item| item.value.clone());
    if let Some(val) = selected_val {
        apply_selected_completion(controller, &val);
    } else {
        apply_completion_generic(controller, completions);
    }
    controller.state_mut().autocomplete.close();
    AutocompleteKeyResult::Handled
}

enum NavDirection {
    Prev,
    Next,
}

fn handle_navigation_key(code: KeyCode, modifiers: KeyModifiers) -> Option<NavDirection> {
    match (code, modifiers) {
        (KeyCode::Up, KeyModifiers::NONE)
        | (KeyCode::Char('p'), KeyModifiers::CONTROL)
        | (KeyCode::BackTab, _)
        | (KeyCode::Tab, KeyModifiers::SHIFT) => Some(NavDirection::Prev),
        (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => Some(NavDirection::Next),
        _ => None,
    }
}

fn dispatch_autocomplete_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
    (code, modifiers): (KeyCode, KeyModifiers),
) -> AutocompleteKeyResult {
    if let Some(dir) = handle_navigation_key(code, modifiers) {
        let state = controller.state_mut();
        match dir {
            NavDirection::Prev => state.autocomplete.select_prev(),
            NavDirection::Next => state.autocomplete.select_next(),
        }
        return AutocompleteKeyResult::Handled;
    }
    match (code, modifiers) {
        (KeyCode::Tab, KeyModifiers::NONE)
        | (KeyCode::Enter, KeyModifiers::NONE)
        | (KeyCode::Right, KeyModifiers::NONE) => handle_accept_key(controller, completions),
        (KeyCode::Esc, KeyModifiers::NONE) => {
            controller.state_mut().autocomplete.close();
            AutocompleteKeyResult::Handled
        }
        _ => AutocompleteKeyResult::NotHandled,
    }
}

pub fn handle_autocomplete_key<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
    key: KeyEvent,
) -> AutocompleteKeyResult {
    handle_autocomplete_key_generic(controller, completions, key)
}

pub fn handle_autocomplete_key_generic<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
    key: KeyEvent,
) -> AutocompleteKeyResult {
    let state = controller.state_mut();
    if !state.autocomplete.visible {
        return AutocompleteKeyResult::NotHandled;
    }
    if key.kind == crossterm::event::KeyEventKind::Release {
        return AutocompleteKeyResult::Handled;
    }
    dispatch_autocomplete_key(controller, completions, (key.code, key.modifiers))
}

pub fn update_autocomplete_state<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) {
    update_autocomplete_state_generic(controller, completions);
}

pub fn update_autocomplete_state_generic<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) {
    let editor = controller.state().editor();
    let text = editor.text();
    let cursor = editor.cursor();

    // Trigger autocomplete when typing a command or file mention
    if (text.starts_with('/') || text.contains('@')) && cursor <= text.len() {
        let matches = completions.complete(text, cursor);
        if !matches.is_empty() {
            controller.state_mut().autocomplete.open(matches);
        } else {
            controller.state_mut().autocomplete.close();
        }
    } else {
        controller.state_mut().autocomplete.close();
    }
}

#[cfg(test)]
mod tests;
