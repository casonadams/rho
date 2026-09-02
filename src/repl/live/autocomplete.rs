use super::navigation::apply_completion_generic;
use crate::repl::interactive::CompletionSet;
use crate::ui::interactive::{CrosstermBackend, TerminalBackend, TerminalController};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum AutocompleteKeyResult {
    Handled,
    NotHandled,
}

pub fn handle_autocomplete_key(
    controller: &mut TerminalController<CrosstermBackend>,
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

    match (key.code, key.modifiers) {
        (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
            state.autocomplete.select_prev();
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            state.autocomplete.select_next();
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
            let selected_val = state.autocomplete.selected_item().map(|item| item.value.clone());
            if let Some(val) = selected_val {
                let editor = state.editor_mut();
                let text = editor.text();
                let cursor = editor.cursor();
                // Replace up to the end of the first word with the selected command + space
                let end = text[cursor..].find(' ').map_or(text.len(), |i| cursor + i);
                let new_text = format!("{} {}", val, &text[end..]);
                editor.set_text(&new_text);
            } else {
                apply_completion_generic(controller, completions);
            }
            controller.state_mut().autocomplete.close();
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            state.autocomplete.close();
            AutocompleteKeyResult::Handled
        }
        _ => AutocompleteKeyResult::NotHandled,
    }
}

pub fn update_autocomplete_state(controller: &mut TerminalController<CrosstermBackend>, completions: &CompletionSet) {
    update_autocomplete_state_generic(controller, completions);
}

pub fn update_autocomplete_state_generic<B: TerminalBackend>(
    controller: &mut TerminalController<B>,
    completions: &CompletionSet,
) {
    let editor = controller.state().editor();
    let text = editor.text();
    let cursor = editor.cursor();

    if text.starts_with('/') && !text[..cursor].contains(' ') {
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
