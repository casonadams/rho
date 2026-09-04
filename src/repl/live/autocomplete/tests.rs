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

#[test]
fn test_autocomplete_pi_exact_contract() {
    let skill1 = ResolvedSkill {
        metadata: SkillMetadata {
            name: "plan".to_string(),
            description: "Planning workflow".to_string(),
            location: "/path".to_string(),
        },
        origin: SkillOrigin::User,
    };
    let skill2 = ResolvedSkill {
        metadata: SkillMetadata {
            name: "spec".to_string(),
            description: "Specification workflow".to_string(),
            location: "/path".to_string(),
        },
        origin: SkillOrigin::User,
    };
    let sources = crate::repl::interactive::CompletionSources::new().with_skills(vec![skill1, skill2]);
    let completions = CompletionSet::from_sources(sources);
    let mut controller = TerminalController::new(MockTerminal, InteractiveState::default()).unwrap();

    // 1. Type "/skil" -> menu opens with "/skill"
    controller.state_mut().editor_mut().set_text("/skil");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    assert_eq!(controller.state().autocomplete.selected_item().unwrap().value, "/skill");

    // 2. Tab accepts "/skill " and closes autocomplete (Pi contract)
    let tab_key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, tab_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().editor().text(), "/skill ");
    assert!(!controller.state().autocomplete.visible);

    // 3. Arrow Down / Up navigates menu when open
    controller.state_mut().editor_mut().set_text("/skill ");
    update_autocomplete_state_generic(&mut controller, &completions);
    assert!(controller.state().autocomplete.visible);
    assert_eq!(controller.state().autocomplete.selected, 0);

    let down_key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
    let res = handle_autocomplete_key_generic(&mut controller, &completions, down_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().autocomplete.selected, 1);

    // 4. Tab applies currently selected skill (spec) and closes
    let res = handle_autocomplete_key_generic(&mut controller, &completions, tab_key);
    assert!(matches!(res, AutocompleteKeyResult::Handled));
    assert_eq!(controller.state().editor().text(), "/skill spec ");
    assert!(!controller.state().autocomplete.visible);
}
