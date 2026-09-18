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
mod tests {
    use crate::repl::interactive::CompletionSet;
    use crate::ui::interactive::{InteractiveState, TerminalBackend, TerminalController};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rho_harness_core::skills::{ResolvedSkill, SkillMetadata, SkillOrigin};
    use std::io;

    use super::*;

    struct MockTerminal;

    impl TerminalBackend for MockTerminal {
        fn set_raw_mode(&mut self, _enabled: bool) -> io::Result<()> {
            Ok(())
        }
        fn size(&self) -> io::Result<(u16, u16)> {
            Ok((80, 24))
        }
        fn hide_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn show_cursor(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn move_up(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_down(&mut self, _rows: usize) -> io::Result<()> {
            Ok(())
        }
        fn move_to_column(&mut self, _col: usize) -> io::Result<()> {
            Ok(())
        }
        fn clear_line(&mut self) -> io::Result<()> {
            Ok(())
        }
        fn write_text(&mut self, _text: &str) -> io::Result<()> {
            Ok(())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn sample_skill_completions(skills: &[(&str, &str)]) -> CompletionSet {
        let list = skills
            .iter()
            .map(|(name, desc)| ResolvedSkill {
                metadata: SkillMetadata {
                    name: name.to_string(),
                    description: desc.to_string(),
                    location: "/path".to_string(),
                    disable_model_invocation: false,
                },
                origin: SkillOrigin::User,
            })
            .collect();
        CompletionSet::from_sources(crate::repl::interactive::CompletionSources::new().with_skills(list))
    }

    fn init_autocomplete_controller(text: &str, completions: &CompletionSet) -> TerminalController<MockTerminal> {
        let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
        controller.state_mut().editor_mut().set_text(text);
        update_autocomplete_state_generic(&mut controller, completions);
        controller
    }

    fn press_key(
        controller: &mut TerminalController<MockTerminal>,
        completions: &CompletionSet,
        code: KeyCode,
    ) -> AutocompleteKeyResult {
        let key = KeyEvent::new(code, KeyModifiers::NONE);
        handle_autocomplete_key_generic(controller, completions, key)
    }

    fn assert_autocomplete_selected(controller: &TerminalController<MockTerminal>, expected: &str) {
        assert!(controller.state().autocomplete.visible);
        assert_eq!(controller.state().autocomplete.selected_item().unwrap().value, expected);
    }

    fn assert_tab_completes(
        controller: &mut TerminalController<MockTerminal>,
        completions: &CompletionSet,
        expected_text: &str,
    ) {
        let res = press_key(controller, completions, KeyCode::Tab);
        assert!(matches!(res, AutocompleteKeyResult::Handled));
        assert_eq!(controller.state().editor().text(), expected_text);
    }

    #[test]
    fn test_autocomplete_pi_contract_command() {
        let completions = sample_skill_completions(&[("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skil", &completions);
        assert_autocomplete_selected(&controller, "/skill");

        assert_tab_completes(&mut controller, &completions, "/skill ");
        assert!(!controller.state().autocomplete.visible);
    }

    #[test]
    fn test_autocomplete_pi_contract_subcommand() {
        let completions = sample_skill_completions(&[("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        assert_eq!(controller.state().autocomplete.selected, 0);

        let res = press_key(&mut controller, &completions, KeyCode::Down);
        assert!(matches!(res, AutocompleteKeyResult::Handled));
        assert_eq!(controller.state().autocomplete.selected, 1);

        assert_tab_completes(&mut controller, &completions, "/skill spec ");
    }

    #[test]
    fn test_autocomplete_arrow_down_navigation() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        press_key(&mut controller, &completions, KeyCode::Down);
        assert_eq!(controller.state().autocomplete.selected, 1);
        press_key(&mut controller, &completions, KeyCode::Down);
        assert_eq!(controller.state().autocomplete.selected, 2);
        press_key(&mut controller, &completions, KeyCode::Down);
        assert_eq!(controller.state().autocomplete.selected, 0);
    }

    #[test]
    fn test_autocomplete_arrow_up_navigation() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        press_key(&mut controller, &completions, KeyCode::Up);
        assert_eq!(controller.state().autocomplete.selected, 2);
        press_key(&mut controller, &completions, KeyCode::Up);
        assert_eq!(controller.state().autocomplete.selected, 1);
        press_key(&mut controller, &completions, KeyCode::Up);
        assert_eq!(controller.state().autocomplete.selected, 0);
    }

    #[test]
    fn test_autocomplete_ctrl_navigation() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        let ctrl_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        handle_autocomplete_key_generic(&mut controller, &completions, ctrl_n);
        assert_eq!(controller.state().autocomplete.selected, 1);
        let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        handle_autocomplete_key_generic(&mut controller, &completions, ctrl_p);
        assert_eq!(controller.state().autocomplete.selected, 0);
    }

    #[test]
    fn test_autocomplete_shift_tab_key() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        let shift_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT);
        let res = handle_autocomplete_key_generic(&mut controller, &completions, shift_tab);
        assert!(matches!(res, AutocompleteKeyResult::Handled));
        assert_eq!(controller.state().autocomplete.selected, 2);
    }

    #[test]
    fn test_autocomplete_backtab_key() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("plan", "Plan"), ("spec", "Spec")]);
        let mut controller = init_autocomplete_controller("/skill ", &completions);
        let backtab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE);
        let res = handle_autocomplete_key_generic(&mut controller, &completions, backtab);
        assert!(matches!(res, AutocompleteKeyResult::Handled));
        assert_eq!(controller.state().autocomplete.selected, 2);
    }

    #[test]
    fn test_autocomplete_ignores_key_release_events() {
        let completions = sample_skill_completions(&[("lean", "Lean"), ("spec", "Spec")]);
        let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();

        controller.state_mut().editor_mut().set_text("/skill ");
        update_autocomplete_state_generic(&mut controller, &completions);
        assert_eq!(
            (
                controller.state().autocomplete.visible,
                controller.state().autocomplete.selected
            ),
            (true, 0)
        );

        let down_release = KeyEvent {
            code: KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            kind: crossterm::event::KeyEventKind::Release,
            state: crossterm::event::KeyEventState::empty(),
        };
        let res = handle_autocomplete_key_generic(&mut controller, &completions, down_release);
        assert!(matches!(res, AutocompleteKeyResult::Handled));
        assert_eq!(controller.state().autocomplete.selected, 0);
    }
}
