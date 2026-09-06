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

#[test]
fn test_autocomplete_theme_command() {
    let completions = CompletionSet::from_sources(crate::repl::interactive::CompletionSources::new());
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("/them");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    assert_eq!(controller.state().autocomplete.selected_item().unwrap().value, "/theme");
}

#[test]
fn test_autocomplete_theme_arguments() {
    let completions = CompletionSet::from_sources(crate::repl::interactive::CompletionSources::new());
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();
    controller.state_mut().editor_mut().set_text("/theme ");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    let items = &controller.state().autocomplete.items;
    assert!(items.iter().any(|i| i.value == "/theme nord") && items.iter().any(|i| i.value == "/theme catppuccin"));
}
