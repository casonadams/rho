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
        (KeyCode::Up, KeyModifiers::NONE)
        | (KeyCode::Char('p'), KeyModifiers::CONTROL)
        | (KeyCode::BackTab, _) => {
            state.autocomplete.select_prev();
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Down, KeyModifiers::NONE)
        | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
            state.autocomplete.select_next();
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Tab, KeyModifiers::NONE) => {
            let editor = state.editor();
            let text = editor.text().trim_end();
            let cursor = editor.cursor();
            let selected_val = state.autocomplete.selected_item().map(|item| item.value.clone());

            let is_exact_prefix_match = if let Some(ref val) = selected_val {
                text == val || text.starts_with(val)
            } else {
                false
            };

            // If user typed a partial prefix (like "/ski" or "/skill pla") that isn't exact yet, Tab completes it!
            // If the text is ALREADY a completed prefix (like "/skill" or "/model"), Tab CYCLES through the menu items!
            if !is_exact_prefix_match && let Some(val) = selected_val {
                let editor = state.editor_mut();
                let text = editor.text();
                if val.starts_with('/') {
                    let mut new_text = val;
                    if !new_text.ends_with(' ') {
                        new_text.push(' ');
                    }
                    new_text.push_str(&text[cursor..]);
                    editor.set_text(&new_text);
                } else {
                    let end = text[cursor..].find(' ').map_or(text.len(), |i| cursor + i);
                    let new_text = format!("{} {}", val, &text[end..]);
                    editor.set_text(&new_text);
                }
                update_autocomplete_state_generic(controller, completions);
            } else {
                state.autocomplete.select_next();
            }
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Right, KeyModifiers::NONE) => {
            let selected_val = state.autocomplete.selected_item().map(|item| item.value.clone());
            if let Some(val) = selected_val {
                let editor = state.editor_mut();
                let text = editor.text();
                let cursor = editor.cursor();
                if val.starts_with('/') {
                    let mut new_text = val;
                    if !new_text.ends_with(' ') {
                        new_text.push(' ');
                    }
                    new_text.push_str(&text[cursor..]);
                    editor.set_text(&new_text);
                } else {
                    let end = text[cursor..].find(' ').map_or(text.len(), |i| cursor + i);
                    let new_text = format!("{} {}", val, &text[end..]);
                    editor.set_text(&new_text);
                }
            } else {
                apply_completion_generic(controller, completions);
            }
            update_autocomplete_state_generic(controller, completions);
            AutocompleteKeyResult::Handled
        }
        (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            state.autocomplete.close();
            AutocompleteKeyResult::Handled
        }
        _ => AutocompleteKeyResult::NotHandled,
    }
}

pub fn update_autocomplete_state(
    controller: &mut TerminalController<CrosstermBackend>,
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

    // Trigger autocomplete whenever typing slash commands, arguments, or file mentions
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
